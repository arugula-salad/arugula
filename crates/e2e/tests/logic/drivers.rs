//! `drivers`: a deploy driver's settings, the plan it makes (the text an
//! owner reads and approves, whose SHA-256 the signed operation holds),
//! whether an operation's plan is a driver's (`planned`, the box's check),
//! and the shape checks beside them. The daemon and the server must make
//! the same text byte for byte: the approval signs its digest.

use arugula_e2e::{
    deploy::Operation,
    drivers::{
        RollBack, Settings, fly_image_ok, ledger_link_ok, plan_digest, plan_text, planned, roll_back_ok,
        setting_fields, settings, source_ok,
    },
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    deploy::DRIVERS,
    harness::{Ctx, Example, Spec, input},
};

pub const FILES: &[Spec] = &[
    Spec { module: "drivers", name: "settings", run: run_settings, make: settings_cases },
    Spec { module: "drivers", name: "setting_fields", run: run_fields, make: fields },
    Spec { module: "drivers", name: "plan_text", run: run_plan, make: plans },
    Spec { module: "drivers", name: "planned", run: run_planned, make: planned_cases },
    Spec { module: "drivers", name: "checks", run: run_checks, make: checks },
];

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn s(kv: &[(&str, &str)]) -> Settings {
    kv.iter().map(|(k, v)| ((*k).into(), (*v).into())).collect()
}

fn image(app: &str, n: char) -> String {
    format!("registry.fly.io/{app}:deployment-1@sha256:{}", n.to_string().repeat(64))
}

fn digest(n: char) -> String {
    format!("sha256:{}", n.to_string().repeat(64))
}

// ---- settings ----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsIn {
    driver: String,
    given: Settings,
}

fn run_settings(_: &Ctx, v: &Value) -> anyhow::Result<Value> {
    let i: SettingsIn = input(v)?;
    Ok(match settings(&i.driver, &i.given) {
        Ok(s) => json!({ "settings": s }),
        Err(e) => json!({ "error": e }),
    })
}

fn settings_cases() -> Example {
    let mut ex = Example::new(
        "drivers",
        "settings",
        run_settings,
        "A driver's settings on an environment, as the environment keeps them: checked, with defaults filled in, \
         or why not (the error text is what the platform answers). A driver without settings (fake, \
         apps-platform, and a name no driver has) takes none. The filled settings are what plan_text writes, so \
         the plan, and the approval over it, bind every one.",
        &[
            "arugula_e2e::drivers::settings",
            "arugula_e2e::drivers::arugula_release::settings",
            "arugula_e2e::deploy::planter::settings",
        ],
    )
    .shapes(
        "driver; given: the settings as given, { name: value }",
        "settings: the filled settings, or error: why not",
    );
    let mut case = |driver: &str, about: &str, given: &[(&str, &str)], ok: bool| {
        let got = ex.computed(&format!("{driver}: {about}"), json!({ "driver": driver, "given": s(given) }));
        assert_eq!(got.get("settings").is_some(), ok, "{driver}: {about}: {got}");
    };
    for d in ["fake", "apps-platform", "nope"] {
        case(d, "none", &[], true);
        case(d, "one it doesn't take", &[("x", "y")], false);
    }
    let c = "chant-fly";
    let ok = [("fly_app", "acme-web"), ("chant_env", "github")];
    let with = |more: &[(&'static str, &'static str)]| [ok.as_slice(), more].concat();
    case(c, "none: it needs fly_app", &[], false);
    case(c, "fly_app alone: it needs chant_env", &[("fly_app", "acme-web")], false);
    case(c, "the two it needs: the rest filled in", &ok, true);
    case(
        c,
        "every setting",
        &with(&[
            ("chant_dir", "ops"),
            ("chant_op", "ship"),
            ("fly_config", "ops/fly.toml"),
            ("chant_gates", "approve-release,ship"),
            ("npm_scripts", "true"),
            ("chant_github", "true"),
            ("chant_digest", "image"),
        ]),
        true,
    );
    case(c, "the repository's root as chant_dir", &with(&[("chant_dir", ".")]), true);
    for (about, bad) in [
        ("a Fly app's name in capitals", vec![("fly_app", "Acme"), ("chant_env", "github")]),
        ("chant_dir out of the repository", with(&[("chant_dir", "../x")])),
        ("chant_dir absolute", with(&[("chant_dir", "/etc")])),
        ("chant_dir reading as a flag", with(&[("chant_dir", "-x")])),
        ("a space in fly_config", with(&[("fly_config", "a b")])),
        ("a semicolon in chant_op", with(&[("chant_op", "re;lease")])),
        ("chant_op reading as a flag", with(&[("chant_op", "-release")])),
        ("a setting it doesn't take", with(&[("token", "x")])),
        ("an empty gate", with(&[("chant_gates", "a,,b")])),
        ("npm_scripts other than true", with(&[("npm_scripts", "yes")])),
        ("chant_github other than true", with(&[("chant_github", "false")])),
        ("chant_digest neither plan nor image", with(&[("chant_digest", "both")])),
        ("chant_env starting with _", vec![("fly_app", "a"), ("chant_env", "_x")]),
    ] {
        case(c, about, &bad, false);
    }
    let r = "arugula-release";
    case(r, "none: patch, daemon", &[], true);
    case(r, "a minor bump of the app", &[("bump", "minor"), ("lane", "app")], true);
    case(r, "a major bump", &[("bump", "major")], true);
    case(r, "a bump it doesn't know", &[("bump", "huge")], false);
    case(r, "a lane it doesn't know", &[("lane", "web")], false);
    case(r, "a setting it doesn't take", &[("tag", "v1")], false);
    let (pr, gl, sh, pm) = ("planter-prod", "planter-go-live", "planter-ship", "planter-promote");
    case(pr, "none: app_dir filled in", &[], true);
    case(
        pr,
        "secrets and a database",
        &[("app_dir", "web/app"), ("health", "/healthz"), ("secrets", "true"), ("postgres", "true")],
        true,
    );
    case(
        pr,
        "a volume, metrics and variables",
        &[("volume", "2Gi"), ("metrics", "/metrics"), ("port_env", "APP_PORT"), ("data_env", "STATE")],
        true,
    );
    case(pr, "no volume", &[("volume", "none")], true);
    for (about, bad) in [
        ("app_dir out of the repository", vec![("app_dir", "../x")]),
        ("app_dir absolute", vec![("app_dir", "/etc")]),
        ("app_dir reading as a flag", vec![("app_dir", "-x")]),
        ("a health path without its slash", vec![("health", "health")]),
        ("secrets other than true", vec![("secrets", "yes")]),
        ("a volume in GB", vec![("volume", "2GB")]),
        ("a volume of 0Gi", vec![("volume", "0Gi")]),
        ("port_env starting with a digit", vec![("port_env", "1PORT")]),
        ("a setting of Ship's", vec![("app", "x")]),
        ("an empty value", vec![("health", "")]),
    ] {
        case(pr, about, &bad, false);
    }
    case(gl, "none", &[], true);
    case(gl, "two custom domains", &[("domains", "shop.example.com www.example.com")], true);
    for (about, d) in [
        ("a domain without a dot", "shop"),
        ("a domain in capitals", "Shop.example.com"),
        ("a domain twice", "a.com a.com"),
        ("a domain starting with -", "-a.com"),
        ("domains comma separated", "a.com,b.com"),
        ("eleven domains", "a.com b.com c.com d.com e.com f.com g.com h.com i.com j.com k.com"),
    ] {
        case(gl, about, &[("domains", d)], false);
    }
    case(gl, "a setting of Ship's", &[("app", "x")], false);
    case(sh, "none: it needs app", &[], false);
    case(sh, "the app alone: port, health and port_env filled in", &[("app", "hello")], true);
    case(sh, "with its secrets", &[("app", "hello"), ("secrets", "STRIPE_KEY,DATABASE_URL")], true);
    for (about, bad) in [
        ("an app in capitals", vec![("app", "Hello")]),
        ("an app of 33", vec![("app", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")]),
        ("a reserved app", vec![("app", "platform")]),
        ("planter's", vec![("app", "planter-x")]),
        ("Flux's", vec![("app", "flux-system")]),
        ("a port under 1024", vec![("app", "hello"), ("port", "80")]),
        ("a port with a leading zero", vec![("app", "hello"), ("port", "08080")]),
        ("a port over 65535", vec![("app", "hello"), ("port", "65536")]),
        ("an empty secret", vec![("app", "hello"), ("secrets", "A,,B")]),
        ("a secret twice", vec![("app", "hello"), ("secrets", "A,A")]),
        ("a setting of Prod's", vec![("app", "hello"), ("volume", "1Gi")]),
    ] {
        case(sh, about, &bad, false);
    }
    case(pm, "its app", &[("app", "hello")], true);
    case(pm, "none: it needs app", &[], false);
    case(pm, "a reserved app", &[("app", "arugula")], false);
    ex
}

// ---- setting fields ----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FieldsIn {
    driver: String,
}

fn run_fields(_: &Ctx, v: &Value) -> anyhow::Result<Value> {
    let i: FieldsIn = input(v)?;
    Ok(json!({ "fields": setting_fields(&i.driver) }))
}

fn fields() -> Example {
    let mut ex = Example::new(
        "drivers",
        "setting_fields",
        run_fields,
        "The settings a driver takes, as a page asks for them when an environment is made (M252), in order: the \
         name, whether it must be given, the default settings fills in, the only values it takes when it's a \
         choice, and what it is, in words.",
        &["arugula_e2e::drivers::setting_fields"],
    )
    .shapes("driver", "fields: [{ name, required, default, choices, about }]");
    for d in DRIVERS.iter().copied().chain(["nope"]) {
        ex.computed(&format!("{d}'s"), json!({ "driver": d }));
    }
    ex
}

// ---- plan text ----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanIn {
    driver: String,
    environment_name: String,
    source_kind: String,
    source_ref: String,
    source_where: String,
    settings: Settings,
    roll_back: Option<RollBack>,
}

fn run_plan(_: &Ctx, v: &Value) -> anyhow::Result<Value> {
    let i: PlanIn = input(v)?;
    let text = plan_text(
        &i.driver,
        &i.environment_name,
        &i.source_kind,
        &i.source_ref,
        &i.source_where,
        &i.settings,
        i.roll_back.as_ref(),
    );
    let op = Operation {
        project: String::new(),
        environment: String::new(),
        environment_name: i.environment_name,
        plan: String::new(),
        source_kind: i.source_kind,
        source_ref: i.source_ref,
        source_where: i.source_where,
        planned: String::new(),
    };
    Ok(json!({ "text": text, "digest": plan_digest(&i.driver, &op, &i.settings, i.roll_back.as_ref()) }))
}

fn filled(driver: &str, given: &[(&str, &str)]) -> Settings {
    settings(driver, &s(given)).unwrap()
}

fn chant_roll_back(app: &str) -> RollBack {
    RollBack {
        deploy: format!("dep_{}", "a".repeat(32)),
        to: [
            ("image", image(app, '0')),
            ("chant_component", "arugula".into()),
            ("chant_env", "github".into()),
            ("chant_digest", "sha256:11".into()),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v))
        .collect(),
    }
}

fn plain_roll_back() -> RollBack {
    RollBack { deploy: format!("dep_{}", "b".repeat(32)), to: Settings::new() }
}

fn plans() -> Example {
    let mut ex = Example::new(
        "drivers",
        "plan_text",
        run_plan,
        "The plan a driver makes for a deploy (or, with roll_back, for rolling back to an earlier one): the text \
         an owner reads before they approve, and its SHA-256, hex, which is the deploy's plan and so in the \
         operation they sign. The daemon (a box recomputes it before it applies) and the server must make it \
         byte for byte the same. It starts arugula deploy plan v1, the driver, the environment's name and the \
         source; apps-platform adds what it pins where; then a line per setting (sorted by name), the roll back \
         and what it goes back to, and what the driver does, in words: chant-fly always, arugula-release unless \
         it's a roll back (it never rolls back), planter's unless it's a roll back of one other than Go live. A \
         source a driver can't deploy is planned with its refusal in the text. Settings here are as settings \
         fills them (drivers/settings).",
        &[
            "arugula_e2e::drivers::plan_text",
            "arugula_e2e::drivers::plan_digest",
            "arugula_e2e::deploy::plan_text",
            "arugula_e2e::deploy::apps_platform::plan_lines",
            "arugula_e2e::drivers::arugula_release::plan_lines",
            "arugula_e2e::deploy::planter::plan_lines",
        ],
    )
    .shapes(
        "driver, environment_name, source_kind (commit or artifact), source_ref, source_where; settings: { name: \
         value }, filled; roll_back: { deploy, to: { name: value } } or null",
        "text: the plan; digest: its SHA-256, hex",
    );
    let plan = |ex: &mut Example,
                about: &str,
                driver: &str,
                env: &str,
                kind: &str,
                r: &str,
                w: &str,
                st: &Settings,
                rb: Option<RollBack>| {
        let got = ex.computed(
            &format!("{driver}: {about}"),
            json!({ "driver": driver, "environment_name": env, "source_kind": kind, "source_ref": r,
                    "source_where": w, "settings": st, "roll_back": rb }),
        );
        assert!(got["text"].as_str().unwrap().starts_with(&format!("arugula deploy plan v1\ndriver {driver}\n")));
    };
    let none = Settings::new();
    plan(&mut ex, "a commit", "fake", "production", "commit", SHA, "acme/app", &none, None);
    plan(&mut ex, "an artifact", "fake", "staging", "artifact", &digest('a'), "registry.example.com/acme", &none, None);
    plan(&mut ex, "a roll back", "fake", "production", "commit", SHA, "acme/app", &none, Some(plain_roll_back()));
    plan(
        &mut ex,
        "a driver no one has: planned as a bare one",
        "nope",
        "production",
        "commit",
        SHA,
        "acme/app",
        &none,
        None,
    );
    let ap = "apps-platform";
    plan(&mut ex, "an app's commit", ap, "production", "commit", SHA, "arugula-salad/hello", &none, None);
    plan(
        &mut ex,
        "a roll back",
        ap,
        "production",
        "commit",
        SHA,
        "arugula-salad/hello",
        &none,
        Some(plain_roll_back()),
    );
    plan(
        &mut ex,
        "refused: an artifact",
        ap,
        "production",
        "artifact",
        &digest('a'),
        "arugula-salad/hello",
        &none,
        None,
    );
    plan(
        &mut ex,
        "refused: a short commit",
        ap,
        "production",
        "commit",
        &SHA[..12],
        "arugula-salad/hello",
        &none,
        None,
    );
    plan(&mut ex, "refused: another org's repository", ap, "production", "commit", SHA, "acme/hello", &none, None);
    let c = "chant-fly";
    let min = filled(c, &[("fly_app", "acme-web"), ("chant_env", "github")]);
    plan(&mut ex, "the settings it needs", c, "production", "commit", SHA, "acme/app", &min, None);
    let every = filled(
        c,
        &[
            ("fly_app", "acme-web"),
            ("chant_env", "github"),
            ("chant_dir", "ops"),
            ("chant_op", "ship"),
            ("fly_config", "ops/fly.toml"),
            ("chant_gates", "approve-release,ship"),
            ("npm_scripts", "true"),
            ("chant_github", "true"),
            ("chant_digest", "image"),
        ],
    );
    plan(&mut ex, "every setting", c, "production", "commit", SHA, "acme/app", &every, None);
    let one_gate = filled(c, &[("fly_app", "acme-web"), ("chant_env", "github"), ("chant_gates", "ship")]);
    plan(&mut ex, "one gate", c, "production", "commit", SHA, "acme/app", &one_gate, None);
    plan(&mut ex, "a roll back", c, "production", "commit", SHA, "acme/app", &min, Some(chant_roll_back("acme-web")));
    plan(
        &mut ex,
        "a roll back with its fly.toml",
        c,
        "production",
        "commit",
        SHA,
        "acme/app",
        &every,
        Some(chant_roll_back("acme-web")),
    );
    let r = "arugula-release";
    let rel = "arugula-salad/arugula";
    plan(&mut ex, "a patch release of the daemon", r, "release", "commit", SHA, rel, &filled(r, &[]), None);
    plan(
        &mut ex,
        "a minor release of the app",
        r,
        "release",
        "commit",
        SHA,
        rel,
        &filled(r, &[("bump", "minor"), ("lane", "app")]),
        None,
    );
    plan(
        &mut ex,
        "a roll back: it does nothing",
        r,
        "release",
        "commit",
        SHA,
        rel,
        &filled(r, &[]),
        Some(plain_roll_back()),
    );
    plan(&mut ex, "refused: an artifact", r, "release", "artifact", &digest('a'), rel, &filled(r, &[]), None);
    plan(&mut ex, "refused: a short commit", r, "release", "commit", &SHA[..12], rel, &filled(r, &[]), None);
    plan(&mut ex, "refused: another repository", r, "release", "commit", SHA, "acme/arugula", &filled(r, &[]), None);
    let pr = "planter-prod";
    let ocarina = "arugula-salad/ocarina";
    plan(&mut ex, "no settings but the default", pr, "production", "commit", SHA, ocarina, &filled(pr, &[]), None);
    plan(
        &mut ex,
        "with secrets and a volume",
        pr,
        "production",
        "commit",
        SHA,
        ocarina,
        &filled(pr, &[("secrets", "true"), ("volume", "1Gi")]),
        None,
    );
    plan(
        &mut ex,
        "every setting",
        pr,
        "production",
        "commit",
        SHA,
        ocarina,
        &filled(
            pr,
            &[
                ("app_dir", "web/app"),
                ("health", "/healthz"),
                ("port_env", "APP_PORT"),
                ("data_env", "STATE"),
                ("secrets", "true"),
                ("postgres", "true"),
                ("metrics", "/metrics"),
                ("volume", "none"),
            ],
        ),
        None,
    );
    plan(
        &mut ex,
        "a roll back: what it does is left out",
        pr,
        "production",
        "commit",
        SHA,
        ocarina,
        &filled(pr, &[]),
        Some(plain_roll_back()),
    );
    plan(
        &mut ex,
        "refused: another org's repository",
        pr,
        "production",
        "commit",
        SHA,
        "acme/ocarina",
        &filled(pr, &[]),
        None,
    );
    let gl = "planter-go-live";
    plan(&mut ex, "no custom domains", gl, "production", "commit", SHA, ocarina, &filled(gl, &[]), None);
    plan(
        &mut ex,
        "two custom domains",
        gl,
        "production",
        "commit",
        SHA,
        ocarina,
        &filled(gl, &[("domains", "shop.example.com www.example.com")]),
        None,
    );
    plan(
        &mut ex,
        "a roll back: Go live says what it does",
        gl,
        "production",
        "commit",
        SHA,
        ocarina,
        &filled(gl, &[]),
        Some(plain_roll_back()),
    );
    plan(
        &mut ex,
        "refused: the list's own repository",
        gl,
        "production",
        "commit",
        SHA,
        "arugula-salad/planter-apps",
        &filled(gl, &[]),
        None,
    );
    let sh = "planter-ship";
    plan(
        &mut ex,
        "an agent's artifact",
        sh,
        "loam",
        "artifact",
        &digest('c'),
        "platform",
        &filled(sh, &[("app", "hello")]),
        None,
    );
    plan(
        &mut ex,
        "with its secrets and port",
        sh,
        "loam",
        "artifact",
        &digest('c'),
        "platform",
        &filled(sh, &[("app", "hello"), ("secrets", "STRIPE_KEY"), ("port", "3000"), ("health", "/up")]),
        None,
    );
    plan(&mut ex, "refused: a commit", sh, "loam", "commit", SHA, "platform", &filled(sh, &[("app", "hello")]), None);
    plan(
        &mut ex,
        "refused: not from the platform's store",
        sh,
        "loam",
        "artifact",
        &digest('c'),
        "arugula-salad/shipped",
        &filled(sh, &[("app", "hello")]),
        None,
    );
    let pm = "planter-promote";
    plan(
        &mut ex,
        "a shipped app",
        pm,
        "cluster",
        "commit",
        SHA,
        "arugula-salad/shipped",
        &filled(pm, &[("app", "hello")]),
        None,
    );
    plan(
        &mut ex,
        "refused: another repository",
        pm,
        "cluster",
        "commit",
        SHA,
        ocarina,
        &filled(pm, &[("app", "hello")]),
        None,
    );
    ex
}

// ---- planned ----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlannedIn {
    driver: String,
    op: String,
    settings: Settings,
    roll_back: Option<RollBack>,
}

fn run_planned(ctx: &Ctx, v: &Value) -> anyhow::Result<Value> {
    let i: PlannedIn = input(v)?;
    Ok(json!({ "ok": planned(&i.driver, &ctx.get(&i.op)?, &i.settings, i.roll_back.as_ref()) }))
}

fn planned_cases() -> Example {
    let mut ex = Example::new(
        "drivers",
        "planned",
        run_planned,
        "A box's check before it applies a deploy: whether the operation the owner signed holds this driver's \
         plan for it, with these settings and this roll back (its plan is plan_text's digest over the \
         operation's environment name and source). Another driver, a changed setting, a roll back for an apply \
         or the other way round, a renamed environment or another source each make another plan, so the box \
         refuses.",
        &["arugula_e2e::drivers::planned"],
    )
    .shapes(
        "driver; op: an operation's label; settings: { name: value }; roll_back: { deploy, to } or null",
        "ok: whether the operation's plan is that one",
    );
    let mk = |driver: &str, name: &str, kind: &str, r: &str, w: &str, st: &Settings, rb: Option<&RollBack>| {
        let mut o = Operation {
            project: "prj_1".into(),
            environment: "env_1".into(),
            environment_name: name.into(),
            plan: String::new(),
            source_kind: kind.into(),
            source_ref: r.into(),
            source_where: w.into(),
            planned: "ab".repeat(32),
        };
        o.plan = plan_digest(driver, &o, st, rb);
        o
    };
    let c = "chant-fly";
    let st = filled(c, &[("fly_app", "acme-web"), ("chant_dir", "ops"), ("chant_env", "github")]);
    let rb = chant_roll_back("acme-web");
    ex.object("chant-fly-apply", &mk(c, "production", "commit", SHA, "acme/app", &st, None));
    ex.object("chant-fly-roll-back", &mk(c, "production", "commit", SHA, "acme/app", &st, Some(&rb)));
    let ship = filled("planter-ship", &[("app", "hello")]);
    ex.object("ship", &mk("planter-ship", "loam", "artifact", &digest('d'), "platform", &ship, None));
    ex.object("fake", &mk("fake", "production", "commit", SHA, "acme/app", &Settings::new(), None));
    let mut renamed = mk(c, "production", "commit", SHA, "acme/app", &st, None);
    renamed.environment_name = "prod".into();
    ex.object("chant-fly-apply-renamed", &renamed);
    let mut swapped = mk("planter-ship", "loam", "artifact", &digest('d'), "platform", &ship, None);
    swapped.source_ref = digest('e');
    ex.object("ship-other-artifact", &swapped);
    let mut other_app = st.clone();
    other_app.insert("fly_app".into(), "acme-other".into());
    let mut scripts = st.clone();
    scripts.insert("npm_scripts".into(), "true".into());
    let mut case = |about: &str, driver: &str, op: &str, st: &Settings, rb: Option<&RollBack>, ok: bool| {
        ex.case(about, json!({ "driver": driver, "op": op, "settings": st, "roll_back": rb }), json!({ "ok": ok }));
    };
    case("chant-fly's apply, as planned", c, "chant-fly-apply", &st, None, true);
    case("checked as another driver's", "fake", "chant-fly-apply", &Settings::new(), None, false);
    case("checked with another Fly app", c, "chant-fly-apply", &other_app, None, false);
    case("checked with install scripts on", c, "chant-fly-apply", &scripts, None, false);
    case("checked as a roll back", c, "chant-fly-apply", &st, Some(&rb), false);
    case("chant-fly's roll back, as planned", c, "chant-fly-roll-back", &st, Some(&rb), true);
    case("the roll back checked as an apply", c, "chant-fly-roll-back", &st, None, false);
    let mut rb2 = rb.clone();
    rb2.to.insert("image".into(), image("acme-web", '9'));
    case("the roll back checked going back to another image", c, "chant-fly-roll-back", &st, Some(&rb2), false);
    case("the environment renamed after it was planned", c, "chant-fly-apply-renamed", &st, None, false);
    case("planter-ship's, as planned", "planter-ship", "ship", &ship, None, true);
    case("checked for another app", "planter-ship", "ship", &filled("planter-ship", &[("app", "other")]), None, false);
    case(
        "checked with another port",
        "planter-ship",
        "ship",
        &filled("planter-ship", &[("app", "hello"), ("port", "3000")]),
        None,
        false,
    );
    case("checked as Promote's", "planter-promote", "ship", &ship, None, false);
    case("another artifact than planned", "planter-ship", "ship-other-artifact", &ship, None, false);
    case("fake's, as planned", "fake", "fake", &Settings::new(), None, true);
    case("fake's checked with a setting", "fake", "fake", &s(&[("x", "y")]), None, false);
    ex
}

// ---- checks ----

// The variants are the checks' names, as the files spell them.
#[allow(clippy::enum_variant_names)]
#[derive(Deserialize)]
#[serde(tag = "check", rename_all = "snake_case", deny_unknown_fields)]
enum ChecksIn {
    SourceOk { driver: String, kind: String, reference: String },
    FlyImageOk { app: String, image: String },
    LedgerLinkOk { place: String, link: String },
    RollBackOk { driver: String, settings: Settings, roll_back: RollBack },
}

fn run_checks(_: &Ctx, v: &Value) -> anyhow::Result<Value> {
    Ok(json!({ "ok": match input::<ChecksIn>(v)? {
        ChecksIn::SourceOk { driver, kind, reference } => source_ok(&driver, &kind, &reference),
        ChecksIn::FlyImageOk { app, image } => fly_image_ok(&app, &image),
        ChecksIn::LedgerLinkOk { place, link } => ledger_link_ok(&place, &link),
        ChecksIn::RollBackOk { driver, settings, roll_back } => roll_back_ok(&driver, &settings, &roll_back),
    } }))
}

fn checks() -> Example {
    let mut ex = Example::new(
        "drivers",
        "checks",
        run_checks,
        "The shapes a driver's source and evidence must have. source_ok: chant-fly and arugula-release deploy a \
         commit by its full 40 lowercase hex (never a branch, a tag or a short id the box resolves), planter's as \
         deploy/planter says, any other driver anything. fly_image_ok: one of Fly's registry's images for the \
         app, by digest (registry.fly.io/<app>[:<tag>]@sha256:<64 hex>). ledger_link_ok: a line of chant's \
         ledger in the repository on GitHub, at a commit. roll_back_ok: the deploy's id (dep_ and 32 hex), each \
         value one printable word starting with a letter or digit, and for chant-fly exactly image, \
         chant_component, chant_env and chant_digest, the image the app's and chant_env the settings'; for any \
         other driver nothing more.",
        &[
            "arugula_e2e::drivers::source_ok",
            "arugula_e2e::drivers::fly_image_ok",
            "arugula_e2e::drivers::ledger_link_ok",
            "arugula_e2e::drivers::roll_back_ok",
        ],
    )
    .shapes(
        "check: source_ok (driver, kind, reference), fly_image_ok (app, image), ledger_link_ok (place, link) or \
         roll_back_ok (driver, settings, roll_back)",
        "ok",
    );
    let mut case = |about: &str, input: Value, ok: bool| ex.case(about, input, json!({ "ok": ok }));
    for (driver, kind, r, ok) in [
        ("chant-fly", "commit", SHA, true),
        ("chant-fly", "commit", "main", false),
        ("chant-fly", "commit", &SHA[..12], false),
        ("chant-fly", "commit", &*SHA.to_uppercase(), false),
        ("chant-fly", "artifact", SHA, false),
        ("arugula-release", "commit", SHA, true),
        ("arugula-release", "commit", "v1.2.3", false),
        ("planter-ship", "artifact", &*digest('b'), true),
        ("planter-ship", "commit", SHA, false),
        ("planter-go-live", "commit", SHA, true),
        ("fake", "commit", "abc", true),
        ("apps-platform", "artifact", "anything", true),
    ] {
        case(
            &format!("{driver} deploying {kind} {r}"),
            json!({ "check": "source_ok", "driver": driver, "kind": kind, "reference": r }),
            ok,
        );
    }
    let img = image("acme-web", 'a');
    for (about, i, ok) in [
        ("the app's image, tagged, by digest", img.clone(), true),
        ("untagged", format!("registry.fly.io/acme-web@sha256:{}", "b".repeat(64)), true),
        ("another app's, by a longer name", img.replace("acme-web", "acme-webx"), false),
        ("another registry", img.replace("registry.fly.io", "evil.io"), false),
        ("by tag only", img.replace("@sha256:", ":"), false),
        ("a digest a digit too long", format!("{img}0"), false),
        ("a space in the tag", img.replace(":deployment-1", ":a b"), false),
        ("an upper-case digest", img.replace(&"a".repeat(64), &"A".repeat(64)), false),
    ] {
        case(&format!("fly image: {about}"), json!({ "check": "fly_image_ok", "app": "acme-web", "image": i }), ok);
    }
    let link = format!("https://github.com/acme/app/blob/{SHA}/github/releases.jsonl#L3");
    for (about, l, ok) in [
        ("a line of the ledger at a commit", link.clone(), true),
        ("another scheme", link.replace("https://", "javascript://"), false),
        ("another repository", link.replace("acme/app", "acme/other"), false),
        ("at a branch", link.replace(SHA, "main"), false),
        ("up a directory", link.replace("github/releases", "../releases"), false),
        ("line 0", link.replace("#L3", "#L0"), false),
        ("line x", link.replace("#L3", "#Lx"), false),
        ("not a .jsonl", link.replace(".jsonl", ".html"), false),
        ("a space in the path", link.replace("github/", "git hub/"), false),
        ("markup after it", format!("{link}\"><script>"), false),
    ] {
        case(
            &format!("ledger link: {about}"),
            json!({ "check": "ledger_link_ok", "place": "acme/app", "link": l }),
            ok,
        );
    }
    let st = filled("chant-fly", &[("fly_app", "acme-web"), ("chant_env", "github")]);
    let rb = chant_roll_back("acme-web");
    let mut elsewhere = rb.clone();
    elsewhere.to.insert("image".into(), image("acme-other", '0'));
    let mut other_env = rb.clone();
    other_env.to.insert("chant_env".into(), "staging".into());
    let mut extra = rb.clone();
    extra.to.insert("extra".into(), "x".into());
    let mut short = rb.clone();
    short.deploy = "dep_abc".into();
    let mut flag = rb.clone();
    flag.to.insert("chant_component".into(), "-rf".into());
    for (about, driver, st, r, ok) in [
        ("chant-fly back to the app's image", "chant-fly", &st, &rb, true),
        ("chant-fly back to another app's image", "chant-fly", &st, &elsewhere, false),
        ("chant-fly back to another ledger environment", "chant-fly", &st, &other_env, false),
        ("chant-fly with a value more", "chant-fly", &st, &extra, false),
        ("chant-fly to a deploy id too short", "chant-fly", &st, &short, false),
        ("chant-fly with a value reading as a flag", "chant-fly", &st, &flag, false),
        ("fake with chant-fly's values", "fake", &Settings::new(), &rb, false),
        ("fake with none", "fake", &Settings::new(), &plain_roll_back(), true),
    ] {
        case(
            &format!("roll back: {about}"),
            json!({ "check": "roll_back_ok", "driver": driver, "settings": st, "roll_back": r }),
            ok,
        );
    }
    ex
}
