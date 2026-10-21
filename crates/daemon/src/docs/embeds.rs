//! Charts and drawings in a section's Markdown (#964), checked before
//! anything is written.
//!
//! Two fences, found the way the web client's Markdown finds them
//! (`web/src/shared/ui/markdown.tsx`, `web/src/shared/ui/embeds.ts`: three backticks or
//! tildes, the language the first word after them, a fence unclosed running
//! to the end, one inside another fence not a fence). A fence starts a
//! line, indented as in a list item; one inside a quote (`> ```chart`) is
//! code on the page too (`markdown.tsx` draws embeds everywhere but in a
//! quote), so the fences checked are exactly the fences drawn:
//!
//! - ```` ```chart ````: a JSON spec, `bar` or `line`: `type`, an optional
//!   `title`, `x` (the key that labels a row), 1 to 4 `y` series and 1 to
//!   200 rows of numbers. A line's x goes up (numbers, ISO dates) or the
//!   spec lists its labels in `order`.
//! - ```` ```svg [alt] ````: one drawing, as SVG text. Its root is `<svg`
//!   with the `xmlns` an `<img>` needs. The client shows it only through
//!   `<img>`, which runs no script, so nothing here sanitises it.
//!
//! And the pictures (#965): `![alt](assets/<name>)` in a line of text. The
//! client draws one as an `<img>` when the target is `assets/` and a plain
//! file name ending in a picture extension, and everywhere else (`http://`,
//! `data:`, `../`) it stays text, never fetched. The daemon checks every
//! image-shaped reference outside a fence and an inline code span (a
//! heading, a list item, a table cell and a quote included: the page draws
//! them all), and wants each to be an asset of this Doc that exists. It
//! errs toward checking: a quote's own fence is code on the page, and
//! the daemon (which reads a fence only where it starts a line) checks
//! what is in it all the same.
//!
//! The rules are `deploy/machine/doc.mjs`'s (`checkChart`, `checkOrdered`,
//! `checkImage`) in Rust, and `web/src/shared/ui/chart.tsx` has them in TypeScript for a chart
//! that arrives some other way. Both run over the files in
//! `tests/fixtures/docs/` (`expect.json` says what each one must say), so
//! they can't drift.

use serde_json::Value;

use super::{
    SECTION_MAX,
    assets::{self, Asset},
};

/// Rows in a chart.
pub const MAX_ROWS: usize = 200;
/// Series in a chart.
pub const MAX_SERIES: usize = 4;
/// The namespace an SVG needs to show in an `<img>`.
const SVG_NS: &str = "http://www.w3.org/2000/svg";

/// A fence found in a body: `lang` is the first word after the mark (a
/// drawing's alt text, the rest of that line, is only the client's), `code`
/// what is inside, and `lines` the span it takes, both fence lines included.
pub struct Fence {
    pub lang: String,
    pub code: String,
    pub lines: std::ops::Range<usize>,
}

/// The text as the client reads it: no carriage returns, tabs as four spaces.
fn normal(md: &str) -> String {
    md.replace('\r', "").replace('\t', "    ")
}

/// `(mark, info)` if `line` opens a fence.
fn opens(line: &str) -> Option<(&str, &str)> {
    let t = line.trim_start();
    let mark = ["```", "~~~"].into_iter().find(|m| t.starts_with(m))?;
    Some((mark, t[3..].trim()))
}

/// Every fence in `lines`, in order.
fn fences_in(lines: &[&str]) -> Vec<Fence> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let Some((mark, info)) = opens(lines[i]) else {
            i += 1;
            continue;
        };
        let start = i;
        let mut code = Vec::new();
        i += 1;
        while i < lines.len() && !lines[i].trim_start().starts_with(mark) {
            code.push(lines[i]);
            i += 1;
        }
        i += 1;
        let lang = info.split_whitespace().next().unwrap_or_default();
        out.push(Fence { lang: lang.to_owned(), code: code.join("\n"), lines: start..i.min(lines.len()) });
    }
    out
}

/// Every fence in a body.
pub fn fences(md: &str) -> Vec<Fence> {
    let md = normal(md);
    fences_in(&md.split('\n').collect::<Vec<_>>())
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether the body's charts, drawings and pictures can be shown, given the
/// Doc's `assets`: the first problem, in one line, or `Ok`.
pub fn check(md: &str, assets: &[Asset]) -> Result<(), String> {
    for f in fences(md) {
        match f.lang.as_str() {
            "chart" => check_chart(&f.code).map(|_| ())?,
            "svg" => check_svg(&f.code)?,
            _ => {}
        }
    }
    let md = normal(md);
    let lines: Vec<&str> = md.split('\n').collect();
    let fenced = fences_in(&lines);
    for (n, line) in lines.iter().enumerate() {
        if fenced.iter().any(|f| f.lines.contains(&n)) {
            continue;
        }
        for r in image_refs(line) {
            check_image(&r.src, assets)?;
        }
    }
    Ok(())
}

/// An image-shaped reference in a line.
struct ImageRef {
    start: usize,
    end: usize,
    alt: String,
    src: String,
}

/// The `![alt](src)` in a line, left to right, skipping inline code. The
/// client's `inline()` finds them the same way (`web/src/shared/ui/markup.tsx`),
/// except that it takes only a target it can show, and this takes any.
fn image_refs(line: &str) -> Vec<ImageRef> {
    let b = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'`' {
            // A span is a pair of backticks with something between them.
            i += match line[i + 1..].find('`') {
                Some(n) if n > 0 => n + 2,
                _ => 1,
            };
        } else if b[i] == b'!' && b.get(i + 1) == Some(&b'[') {
            match image_at(line, i) {
                Some(r) => {
                    i = r.end;
                    out.push(r);
                }
                None => i += 1,
            }
        } else {
            i += 1;
        }
    }
    out
}

/// The reference starting at the `!` at `at`, if there is one.
fn image_at(line: &str, at: usize) -> Option<ImageRef> {
    let rest = &line[at + 2..];
    let close = rest.find(']')?;
    let inner = rest[close + 1..].strip_prefix('(')?;
    let end = inner.find(')')?;
    let src = &inner[..end];
    if src.contains(char::is_whitespace) {
        return None;
    }
    Some(ImageRef { start: at, end: at + 2 + close + 2 + end + 1, alt: rest[..close].to_owned(), src: src.to_owned() })
}

/// The picture's name, if `src` is `assets/` and a plain file name with a
/// picture extension: what the client shows.
fn asset_name(src: &str) -> Option<&str> {
    src.strip_prefix("assets/").filter(|n| assets::is_name(n) && assets::named_mime(n).is_some())
}

/// A reference's target must be a picture of this Doc.
fn check_image(src: &str, assets: &[Asset]) -> Result<(), String> {
    let bad = |why: String| format!("image {}: {why}", one_line(src));
    let Some(name) = src.strip_prefix("assets/").filter(|n| assets::is_name(n)) else {
        return Err(bad(
            "must be assets/<file>, a plain file name in this Doc's assets (send one with doc asset first)".into(),
        ));
    };
    if asset_name(src).is_none() {
        return Err(bad(format!("must be one of {}", assets::extensions())));
    }
    if !assets.iter().any(|a| a.name == name) {
        return Err(bad("no such asset in this Doc: send it with doc asset first".into()));
    }
    Ok(())
}

/// The body as plain Markdown: a chart is a line saying what it is, a
/// drawing is `[drawing]` and a picture `[image: alt]` (the Doc's `text()`,
/// which `history` and `search` read).
pub fn plain(md: &str) -> String {
    let md = normal(md);
    let lines: Vec<&str> = md.split('\n').collect();
    let mut out: Vec<String> = Vec::new();
    let mut at = 0;
    for f in fences_in(&lines) {
        out.extend(lines[at..f.lines.start].iter().map(|l| stand_in(l)));
        match f.lang.as_str() {
            "chart" => out.push(summary(&f.code)),
            "svg" => out.push("[drawing]".to_owned()),
            // Code is as it was written, pictures in it included.
            _ => out.extend(lines[f.lines.clone()].iter().map(|l| (*l).to_owned())),
        }
        at = f.lines.end;
    }
    out.extend(lines[at.min(lines.len())..].iter().map(|l| stand_in(l)));
    out.join("\n")
}

/// A line of text with each picture the client would show as `[image: alt]`.
fn stand_in(line: &str) -> String {
    let (mut out, mut at) = (String::new(), 0);
    for r in image_refs(line).into_iter().filter(|r| asset_name(&r.src).is_some()) {
        out.push_str(&line[at..r.start]);
        match r.alt.trim() {
            "" => out.push_str("[image]"),
            alt => out.push_str(&format!("[image: {}]", one_line(alt))),
        }
        at = r.end;
    }
    out.push_str(&line[at..]);
    out
}

/// `[chart: Sales by month (bar, 3 rows)]`; `[chart]` for one that can't be
/// read.
fn summary(code: &str) -> String {
    let Ok(spec) = check_chart(code) else { return "[chart]".into() };
    let n = spec["rows"].as_array().map_or(0, Vec::len);
    let rows = format!("{n} row{}", if n == 1 { "" } else { "s" });
    let kind = spec["type"].as_str().unwrap_or_default();
    match spec["title"].as_str().filter(|t| !t.is_empty()) {
        Some(t) => format!("[chart: {} ({kind}, {rows})]", one_line(t)),
        None => format!("[chart: {kind}, {rows}]"),
    }
}

/// A chart's spec, if it can be drawn; else one line saying what to fix.
pub fn check_chart(source: &str) -> Result<Value, String> {
    let bad = |why: String| format!("chart: {why}");
    let spec: Value =
        serde_json::from_str(source).map_err(|e| bad(format!("not JSON ({})", one_line(&e.to_string()))))?;
    let Some(obj) = spec.as_object() else { return Err(bad("must be a JSON object".into())) };
    match obj.get("type").and_then(Value::as_str) {
        Some("bar" | "line") => {}
        _ => {
            let got = obj.get("type").map_or("nothing".to_owned(), |t| one_line(&t.to_string()));
            return Err(bad(format!("type must be \"bar\" or \"line\", got {got}")));
        }
    }
    if obj.get("title").is_some_and(|t| !t.is_string()) {
        return Err(bad("title must be a string".into()));
    }
    let x = match obj.get("x").and_then(Value::as_str) {
        Some(x) if !x.is_empty() => x,
        _ => return Err(bad("x must name the key that labels each row".into())),
    };
    let single = [obj.get("y").cloned().unwrap_or(Value::Null)];
    let ys = obj.get("y").and_then(Value::as_array).map_or(&single[..], Vec::as_slice);
    let y_ok = (1..=MAX_SERIES).contains(&ys.len()) && ys.iter().all(|y| y.as_str().is_some_and(|s| !s.is_empty()));
    if !y_ok {
        return Err(bad(format!("y must be a key, or a list of 1 to {MAX_SERIES} keys")));
    }
    let rows = match obj.get("rows").and_then(Value::as_array) {
        Some(r) if !r.is_empty() => r,
        _ => return Err(bad("rows must be a non-empty list".into())),
    };
    if rows.len() > MAX_ROWS {
        return Err(bad(format!("{} rows; the cap is {MAX_ROWS}", rows.len())));
    }
    for (i, row) in rows.iter().enumerate() {
        let n = i + 1;
        let Some(row) = row.as_object() else { return Err(bad(format!("row {n} must be an object"))) };
        if !row.get(x).is_some_and(|v| v.is_string() || v.is_number()) {
            return Err(bad(format!("row {n} has no {} label", Value::from(x))));
        }
        for y in ys {
            let y = y.as_str().unwrap_or_default();
            if !row.get(y).and_then(Value::as_f64).is_some_and(f64::is_finite) {
                return Err(bad(format!("row {n}: {} must be a number", Value::from(y))));
            }
        }
    }
    if obj["type"] == "line" {
        check_ordered(obj, x, rows).map_err(bad)?;
    }
    Ok(spec)
}

/// A line joins its points in row order, so the order has to mean something:
/// numbers going up, dates going forward, or labels the spec listed in
/// `order`. Bars have no such rule.
fn check_ordered(spec: &serde_json::Map<String, Value>, x: &str, rows: &[Value]) -> Result<(), String> {
    let xs: Vec<&Value> = rows.iter().map(|r| &r[x]).collect();
    let rising = |keys: &[f64], what: &str| {
        for i in 1..keys.len() {
            if keys[i] <= keys[i - 1] {
                return Err(format!(
                    "a line chart's x must go up: row {} ({}) does not come after row {} ({what})",
                    i + 1,
                    one_line(&xs[i].to_string()),
                    i
                ));
            }
        }
        Ok(())
    };
    if xs.iter().all(|x| x.is_number()) {
        return rising(&xs.iter().map(|x| x.as_f64().unwrap_or_default()).collect::<Vec<_>>(), "numbers");
    }
    let dates: Option<Vec<f64>> = xs.iter().map(|x| x.as_str().and_then(iso_instant)).collect();
    if let Some(dates) = dates {
        return rising(&dates, "dates");
    }
    let Some(order) = spec.get("order") else {
        return Err("a line chart's x must be numbers, dates (2026-01-31), or labels with \"order\": [...] listing them in order".into());
    };
    let order: Vec<&str> = match order.as_array().map(|o| o.iter().map(Value::as_str).collect::<Option<Vec<_>>>()) {
        Some(Some(o)) => o,
        _ => return Err("order must be a list of the x labels, in order".into()),
    };
    let mut at = Vec::new();
    for (i, x) in xs.iter().enumerate() {
        match x.as_str().and_then(|s| order.iter().position(|o| *o == s)) {
            Some(p) => at.push(p as f64),
            None => return Err(format!("row {} ({}) is not in order", i + 1, one_line(&x.to_string()))),
        }
    }
    rising(&at, "order")
}

/// Seconds since the epoch for `2026-01-31`, `2026-01-31T10:00:00Z` and
/// `2026-01-31 10:00:00.5+02:00`; a time with no zone is UTC. `None` for
/// anything else, or a date that isn't one.
fn iso_instant(s: &str) -> Option<f64> {
    let b = s.as_bytes();
    let num = |from: usize, len: usize| -> Option<i64> {
        let d = b.get(from..from + len)?;
        d.iter().all(u8::is_ascii_digit).then(|| d.iter().fold(0, |n, c| n * 10 + i64::from(c - b'0')))
    };
    if b.get(4) != Some(&b'-') || b.get(7) != Some(&b'-') {
        return None;
    }
    let (y, m, d) = (num(0, 4)?, num(5, 2)?, num(8, 2)?);
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days_in = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if !(1..=12).contains(&m) || d < 1 || d > days_in[m as usize - 1] {
        return None;
    }
    // Days since 1970-01-01 (Howard Hinnant's days_from_civil).
    let yy = if m <= 2 { y - 1 } else { y };
    let era = yy.div_euclid(400);
    let yoe = yy - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let mut secs = ((era * 146_097 + doe - 719_468) * 86_400) as f64;
    let rest = &s[10..];
    if rest.is_empty() {
        return Some(secs);
    }
    let rest = rest.strip_prefix(['T', ' '])?;
    let (time, zone) = match rest.find(['Z', '+', '-']) {
        Some(at) => rest.split_at(at),
        None => (rest, ""),
    };
    // HH:MM, then optionally :SS and a fraction.
    let t = time.as_bytes();
    let two = |from: usize| -> Option<f64> {
        let d = t.get(from..from + 2)?;
        d.iter().all(u8::is_ascii_digit).then(|| f64::from((d[0] - b'0') * 10 + (d[1] - b'0')))
    };
    if t.len() < 5 || t[2] != b':' {
        return None;
    }
    let (hh, mm) = (two(0)?, two(3)?);
    let mut ss = 0.0;
    if t.len() > 5 {
        if t[5] != b':' {
            return None;
        }
        ss = two(6)?;
        if t.len() > 8 {
            let frac = time[8..].strip_prefix('.')?;
            if frac.is_empty() || !frac.bytes().all(|c| c.is_ascii_digit()) {
                return None;
            }
            ss += format!("0.{frac}").parse::<f64>().ok()?;
        }
    }
    if hh > 23.0 || mm > 59.0 || ss >= 60.0 {
        return None;
    }
    secs += hh * 3600.0 + mm * 60.0 + ss;
    if let Some(sign) = zone.chars().next().filter(|c| *c != 'Z') {
        let digits = zone[1..].replacen(':', "", 1);
        if digits.len() != 4 || !digits.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        let (oh, om): (f64, f64) = (digits[..2].parse().ok()?, digits[2..].parse().ok()?);
        if oh > 23.0 || om > 59.0 {
            return None;
        }
        secs -= if sign == '-' { -1.0 } else { 1.0 } * (oh * 3600.0 + om * 60.0);
    } else if zone.len() > 1 {
        return None;
    }
    Some(secs)
}

/// A drawing's text, if it can be shown through `<img>`; else one line
/// saying what to fix.
pub fn check_svg(svg: &str) -> Result<(), String> {
    let bad = |why: String| format!("svg: {why}");
    if svg.len() > SECTION_MAX {
        return Err(bad(format!("{} bytes; the cap is {SECTION_MAX} (64 KB)", svg.len())));
    }
    let mut rest = svg.trim();
    if rest.is_empty() {
        return Err(bad("the fence is empty: put one <svg> drawing in it".into()));
    }
    // An XML declaration and comments may come before the root.
    loop {
        let after = if rest.starts_with("<?") {
            rest.find("?>").map(|e| &rest[e + 2..])
        } else if rest.starts_with("<!--") {
            rest.find("-->").map(|e| &rest[e + 3..])
        } else {
            break;
        };
        match after {
            Some(a) => rest = a.trim_start(),
            None => break,
        }
    }
    let root = rest.strip_prefix("<svg").filter(|r| r.starts_with(|c: char| c.is_whitespace() || c == '>' || c == '/'));
    let Some(root) = root else {
        return Err(bad("a drawing must be one <svg> element: its text has to start with <svg".into()));
    };
    let tag = &root[..root.find('>').unwrap_or(root.len())];
    if !tag.contains(&format!("xmlns=\"{SVG_NS}\"")) && !tag.contains(&format!("xmlns='{SVG_NS}'")) {
        return Err(bad(format!("the <svg> needs xmlns=\"{SVG_NS}\", or the picture shows nothing")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::*;

    fn dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/docs")
    }

    fn expect() -> Value {
        serde_json::from_str(&std::fs::read_to_string(dir().join("expect.json")).unwrap()).unwrap()
    }

    /// The Doc the examples are checked against: the assets expect.json names.
    fn assets() -> Vec<Asset> {
        let named = |n: &str| Asset { name: n.to_owned(), mime: assets::named_mime(n).unwrap(), size: 1, rev: 1 };
        expect()["assets"].as_array().unwrap().iter().map(|n| named(n.as_str().unwrap())).collect()
    }

    fn check(md: &str) -> Result<(), String> {
        super::check(md, &assets())
    }

    fn files(sub: &str) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir().join(sub))
            .unwrap()
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .filter(|n| n.ends_with(".md"))
            .map(|n| if sub.is_empty() { n } else { format!("{sub}/{n}") })
            .collect();
        names.sort();
        names
    }

    fn read(name: &str) -> String {
        std::fs::read_to_string(dir().join(name)).unwrap()
    }

    #[test]
    fn the_good_examples_pass_and_read_as_text() {
        let want = &expect()["good"];
        let names = files("");
        assert_eq!(names, want.as_object().unwrap().keys().cloned().collect::<Vec<_>>(), "expect.json lists them all");
        for name in names {
            let md = read(&name);
            assert_eq!(check(&md), Ok(()), "{name}");
            assert_eq!(plain(&md), want[&name].as_str().unwrap(), "{name}");
        }
    }

    #[test]
    fn every_bad_example_is_refused_with_a_line_naming_the_problem() {
        let want = &expect()["bad"];
        let names = files("bad");
        assert_eq!(names, want.as_object().unwrap().keys().cloned().collect::<Vec<_>>(), "expect.json lists them all");
        for name in names {
            let e = check(&read(&name)).expect_err(&name);
            let says = want[&name].as_str().unwrap();
            assert!(e.contains(says) && !e.contains('\n'), "{name}: {e}");
        }
    }

    fn chart(spec: &Value) -> String {
        format!("```chart\n{spec}\n```\n")
    }

    #[test]
    fn each_chart_rule_is_enforced() {
        let rows = |n: usize| (0..n).map(|i| json!({ "m": i, "a": 1, "b": 2 })).collect::<Vec<_>>();
        let line = |rows: Value| json!({ "type": "line", "x": "m", "y": "a", "rows": rows });
        for (spec, says) in [
            (json!({ "type": "bar", "x": "m", "y": ["a", "b", "a", "b", "a"], "rows": rows(2) }), "1 to 4 keys"),
            (json!({ "type": "bar", "x": "m", "y": [], "rows": rows(2) }), "1 to 4 keys"),
            (json!({ "type": "bar", "x": "m", "y": "a", "rows": [] }), "non-empty list"),
            (json!({ "type": "bar", "x": "m", "y": "a", "rows": rows(201) }), "201 rows; the cap is 200"),
            (json!({ "type": "bar", "x": "m", "y": "a", "rows": [{ "m": "x", "a": "7" }] }), "must be a number"),
            (json!({ "type": "bar", "x": "m", "y": "a", "rows": [{ "m": "x", "a": null }] }), "must be a number"),
            (json!({ "type": "bar", "x": "m", "y": "a", "rows": [{ "a": 1 }] }), "has no \"m\" label"),
            (json!({ "type": "bar", "x": "m", "y": "a", "rows": [3] }), "row 1 must be an object"),
            (json!({ "type": "bar", "x": "m", "y": "a", "title": 4, "rows": rows(1) }), "title must be a string"),
            (json!({ "type": "bar", "y": "a", "rows": rows(1) }), "x must name"),
            (json!([1]), "must be a JSON object"),
            (line(json!([{ "m": 1, "a": 1 }, { "m": 1, "a": 2 }])), "x must go up: row 2 (1)"),
            (line(json!([{ "m": 3, "a": 1 }, { "m": 2, "a": 2 }])), "x must go up"),
            (line(json!([{ "m": "2026-02-01", "a": 1 }, { "m": "2026-01-31", "a": 2 }])), "x must go up"),
            (
                line(json!([{ "m": "2026-01-31T10:00:00Z", "a": 1 }, { "m": "2026-01-31T10:00:00+02:00", "a": 2 }])),
                "x must go up",
            ),
            (line(json!([{ "m": "b", "a": 1 }, { "m": "a", "a": 2 }])), "must be numbers, dates"),
            (
                json!({ "type": "line", "x": "m", "y": "a", "order": ["a", "b"], "rows": [{ "m": "b", "a": 1 }, { "m": "a", "a": 2 }] }),
                "x must go up",
            ),
            (
                json!({ "type": "line", "x": "m", "y": "a", "order": ["a"], "rows": [{ "m": "a", "a": 1 }, { "m": "b", "a": 2 }] }),
                "row 2 (\"b\") is not in order",
            ),
            (
                json!({ "type": "line", "x": "m", "y": "a", "order": "ab", "rows": [{ "m": "a", "a": 1 }] }),
                "order must be a list",
            ),
        ] {
            let e = check(&chart(&spec)).expect_err(&spec.to_string());
            assert!(e.starts_with("chart: ") && e.contains(says) && !e.contains('\n'), "{spec}: {e}");
        }
        // What passes: 4 series, 200 rows, bars in any order, dates with
        // times and zones going forward, a number as a label, a line over
        // labels in `order`, and an invalid date taken as a label.
        let ok = |spec: Value| assert_eq!(check(&chart(&spec)), Ok(()), "{spec}");
        ok(json!({ "type": "bar", "x": "m", "y": ["a", "b", "a", "b"], "rows": rows(200) }));
        ok(json!({ "type": "bar", "x": "m", "y": "a", "rows": [{ "m": "z", "a": -3 }, { "m": "b", "a": 0.5 }] }));
        ok(line(
            json!([{ "m": "2026-01-31", "a": 1 }, { "m": "2026-02-01 08:00", "a": 2 }, { "m": "2026-02-01T10:00:00+01:00", "a": 3 }]),
        ));
        ok(line(json!([{ "m": -2, "a": 1 }, { "m": 0.5, "a": 2 }])));
        ok(
            json!({ "type": "line", "x": "m", "y": "a", "order": ["2026-13-01", "2026-13-02"], "rows": [{ "m": "2026-13-01", "a": 1 }, { "m": "2026-13-02", "a": 2 }] }),
        );
    }

    #[test]
    fn a_drawing_needs_an_svg_root_and_stays_under_the_cap() {
        let svg = |inner: &str| format!("<svg xmlns=\"{SVG_NS}\" viewBox=\"0 0 10 10\">{inner}</svg>");
        for good in [
            svg("<rect/>"),
            format!("<?xml version=\"1.0\"?>\n<!-- a box -->\n{}", svg("")),
            format!("<svg xmlns='{SVG_NS}'/>"),
            svg("<script>alert(1)</script>"),
        ] {
            assert_eq!(check_svg(&good), Ok(()), "{good}");
        }
        for (bad, says) in [
            ("".to_owned(), "empty"),
            ("<div>no</div>".to_owned(), "start with <svg"),
            ("<svgfoo/>".to_owned(), "start with <svg"),
            ("<g></g><svg xmlns=\"http://www.w3.org/2000/svg\"/>".to_owned(), "start with <svg"),
            ("<svg viewBox=\"0 0 1 1\"></svg>".to_owned(), "needs xmlns"),
            // The namespace has to be on the root, not further in.
            (format!("<svg><g xmlns=\"{SVG_NS}\"/></svg>"), "needs xmlns"),
            (svg(&"x".repeat(SECTION_MAX)), "the cap is 65536"),
        ] {
            let e = check_svg(&bad).expect_err(&bad);
            assert!(e.starts_with("svg: ") && e.contains(says) && !e.contains('\n'), "{e}");
        }
        // Through a fence, with its alt text after the language.
        assert_eq!(check(&format!("```svg A box\n{}\n```", svg("<rect/>"))), Ok(()));
        assert!(check("```svg\n<p>hi</p>\n```").is_err());
    }

    #[test]
    fn only_a_real_fence_is_checked() {
        // In inline code, in another fence, or in another language: not a
        // chart or a drawing.
        let md = "A `chart` and ```svg``` inline.\n\n~~~text\n```chart\nnot json\n```\n~~~\n\n```json\n{}\n```\n";
        assert_eq!(check(md), Ok(()));
        assert_eq!(plain(md), md);
        // Tildes work, and an unclosed fence runs to the end.
        assert!(check("~~~chart\nnope\n~~~").is_err());
        assert!(check("text\n```chart\n{").is_err());
        // Indented, as in a list item.
        assert!(check("- item\n\n    ```svg\n    <p/>\n    ```").is_err());
    }

    #[test]
    fn a_picture_is_checked_wherever_the_page_draws_it() {
        // A text line's pictures, however many and wherever in it.
        assert_eq!(check("a ![x](assets/chart.png) b ![y](assets/photo.jpg)"), Ok(()));
        let e = check("![x](assets/chart.png) ![y](assets/gone.png)").unwrap_err();
        assert!(e.starts_with("image assets/gone.png: no such asset") && !e.contains('\n'), "{e}");
        // A name is exact, and the extension has to be a picture's.
        for (md, says) in [
            ("![x](assets/Chart.png)", "no such asset"),
            ("![x](assets/chart.PNG)", "must be one of"),
            ("![x](assets/chart)", "must be one of"),
            ("![x](assets/chart.png.html)", "must be one of"),
            ("![x](assets/.chart.png)", "must be assets/<file>"),
            ("![x](/assets/chart.png)", "must be assets/<file>"),
            ("![x](./assets/chart.png)", "must be assets/<file>"),
            ("![x](assets/../chart.png)", "must be assets/<file>"),
            ("![x]()", "must be assets/<file>"),
            ("![x](//example.com/a.png)", "must be assets/<file>"),
            // Bold, a link around it, a deeper list: the page draws or doesn't, and the daemon checks.
            ("**![x](assets/gone.png)**", "no such asset"),
            ("[![x](assets/gone.png)](https://example.com)", "no such asset"),
            ("1. one\n\n   - two\n\n     ![x](assets/gone.png)", "no such asset"),
            // A quote's own fence is code on the page; the daemon errs toward checking.
            ("> ```\n> ![x](assets/gone.png)\n> ```", "no such asset"),
        ] {
            let e = check(md).expect_err(md);
            assert!(e.contains(says) && !e.contains('\n'), "{md}: {e}");
        }
        // What is not an image: a link, a title after the target, text.
        for md in [
            "[a link](https://example.com/a.png)",
            "![x](assets/gone.png \"title\")",
            "![x] (assets/gone.png)",
            "!\\[x](assets/gone.png)",
            "``![x](assets/gone.png)``",
            "`![x](assets/gone.png)` and ``more``",
            "```\n![x](assets/gone.png)\n```",
            "- item\n\n    ~~~\n    ![x](assets/gone.png)\n    ~~~",
        ] {
            assert_eq!(check(md), Ok(()), "{md}");
        }
        // An unclosed fence runs to the end, pictures in it included.
        assert_eq!(check("text\n```\n![x](assets/gone.png)"), Ok(()));
        // A picture after a code span is still found.
        assert!(check("`a` ![x](assets/gone.png)").is_err());
        // Unicode text around a picture is no trouble.
        assert_eq!(check("é `ü` ![ñ](assets/chart.png) 日本"), Ok(()));
    }

    #[test]
    fn plain_text_says_what_a_chart_and_a_drawing_are() {
        let spec =
            json!({ "type": "bar", "title": "Sales\nby month", "x": "m", "y": "a", "rows": [{ "m": 1, "a": 2 }] });
        let md = format!(
            "Before.\n\n{}\nBetween.\n\n```svg Flow\n<svg xmlns=\"{SVG_NS}\"/>\n```\n\n```chart\n{{\"type\":\"line\",\"x\":\"m\",\"y\":\"a\",\"rows\":[{{\"m\":1,\"a\":1}},{{\"m\":2,\"a\":2}}]}}\n```\nAfter.",
            chart(&spec)
        );
        assert_eq!(
            plain(&md),
            "Before.\n\n[chart: Sales by month (bar, 1 row)]\n\nBetween.\n\n[drawing]\n\n[chart: line, 2 rows]\nAfter."
        );
        // One that can't be read is just a chart.
        assert_eq!(plain("```chart\nnope\n```"), "[chart]");
        // A picture is its alt text; one the page wouldn't draw stays as written.
        assert_eq!(
            plain(
                "See ![the  flow\nchart](assets/a.png) and ![x](assets/a.png), ![](assets/a.png) or ![y](http://e.com/a.png)"
            ),
            "See ![the  flow\nchart](assets/a.png) and [image: x], [image] or ![y](http://e.com/a.png)"
        );
    }
}
