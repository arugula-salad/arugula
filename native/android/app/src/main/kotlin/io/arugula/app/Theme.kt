// Arugula's look, as the web draws it and the iPhone's Theme.swift takes it,
// on Material 3: Catppuccin Mocha on a near-black void, Big Shoulders Display
// for titles and numbers, Instrument Sans for text, JetBrains Mono for labels
// and ids, and cards tinted by a tone with a bar down their left edge.

package io.arugula.app

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontVariation
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

object Palette {
    val void = Color(0xFF11111B)
    val bg = Color(0xFF1E1E2E)
    val surface = Color(0xFF181825)
    val raised = Color(0xFF313244)
    val fg = Color(0xFFCDD6F4)
    val subtle = Color(0xFFA6ADC8)
    val dim = Color(0xFF7F849C)
    val muted = Color(0xFF585B70)
    val accent = Color(0xFFCBA6F7)
    val ok = Color(0xFFA6E3A1)
    val warn = Color(0xFFF9E2AF)
    val danger = Color(0xFFF38BA8)
    val info = Color(0xFF89B4FA)
    val peach = Color(0xFFFAB387)
    val teal = Color(0xFF94E2D5)
    val pink = Color(0xFFF5C2E7)
    val merged = Color(0xFFB4BEFE)
}

/** The three faces, variable fonts at the weights asked for. */
object Face {
    private fun family(res: Int, vararg weights: Int) = FontFamily(
        weights.map { w -> Font(res, FontWeight(w), variationSettings = FontVariation.Settings(FontVariation.weight(w))) },
    )

    val display = family(R.font.big_shoulders, 700, 800, 900)
    val ui = family(R.font.instrument_sans, 400, 500, 600, 700)
    val mono = family(R.font.jetbrains_mono, 400, 500, 700)

    fun display(size: TextUnit, weight: FontWeight = FontWeight.ExtraBold) = TextStyle(fontFamily = display, fontWeight = weight, fontSize = size, letterSpacing = 0.4.sp)
    fun ui(size: TextUnit, weight: FontWeight = FontWeight.Normal) = TextStyle(fontFamily = ui, fontWeight = weight, fontSize = size)
    fun mono(size: TextUnit, weight: FontWeight = FontWeight.Normal) = TextStyle(fontFamily = mono, fontWeight = weight, fontSize = size)
}

@Composable
fun ArugulaTheme(content: @Composable () -> Unit) {
    MaterialTheme(
        colorScheme = darkColorScheme(
            primary = Palette.accent,
            onPrimary = Palette.void,
            secondary = Palette.info,
            secondaryContainer = Palette.raised,
            onSecondaryContainer = Palette.fg,
            background = Palette.void,
            onBackground = Palette.fg,
            surface = Palette.void,
            onSurface = Palette.fg,
            surfaceVariant = Palette.surface,
            onSurfaceVariant = Palette.subtle,
            surfaceContainer = Palette.void,
            surfaceContainerHigh = Palette.surface,
            surfaceContainerHighest = Palette.raised,
            outline = Palette.raised,
            outlineVariant = Palette.raised,
            error = Palette.danger,
        ),
        typography = MaterialTheme.typography.let { t ->
            t.copy(
                bodyLarge = Face.ui(16.sp), bodyMedium = Face.ui(14.sp), bodySmall = Face.ui(12.sp),
                labelLarge = Face.ui(14.sp, FontWeight.SemiBold), labelMedium = Face.mono(11.sp, FontWeight.Medium),
                labelSmall = Face.mono(10.sp, FontWeight.Medium), titleLarge = Face.display(26.sp),
                titleMedium = Face.ui(16.sp, FontWeight.SemiBold), titleSmall = Face.ui(14.sp, FontWeight.SemiBold),
            )
        },
        content = content,
    )
}

/** A card's tone, as the web's cards and the swarm's take them. */
enum class Tone(val color: Color) {
    ACCENT(Palette.accent), OK(Palette.ok), WARN(Palette.warn), DANGER(Palette.danger), INFO(Palette.info),
    MUTED(Palette.muted), PEACH(Palette.peach), TEAL(Palette.teal),
}

/** How a card or a session stands, most urgent first (the Overview's words).
 * [WAITS]: it, or its question, waits on someone else (Arugula #1392). */
enum class Standing(val word: String, val tone: Tone) {
    NEEDS("needs you", Tone.ACCENT),
    ASKS("asks you", Tone.ACCENT),
    FAILED("failed", Tone.DANGER),
    CHECKS_FAILING("checks failing", Tone.DANGER),
    WAITS("asks someone else", Tone.INFO),
    QUIET("quiet", Tone.WARN),
    WORKING("working", Tone.OK),
    CHECKS_RUNNING("checks running", Tone.INFO),
    REVIEW("in review", Tone.INFO),
    IDLE("", Tone.MUTED),
    ;

    /** Its word, naming whom it waits on when that's someone else: `asks Aaron`, or `asks someone` where that isn't known. */
    fun word(asks: String?): String = if (this == WAITS) "asks ${asks?.takeIf { it.isNotEmpty() } ?: "someone"}" else word
}

/** A section's label, as the swarm's: small mono capitals, dim. */
@Composable
fun Caps(text: String, modifier: Modifier = Modifier, color: Color = Palette.dim) {
    Text(text.uppercase(), modifier, style = Face.mono(11.sp, FontWeight.Medium).copy(letterSpacing = 1.sp), color = color, maxLines = 1, overflow = TextOverflow.Ellipsis)
}

/** A card: the surface tinted by its tone, a 1px edge in it, and a 3px bar down the left. Live work glows. */
@Composable
fun ToneCard(tone: Tone, modifier: Modifier = Modifier, glow: Boolean = false, content: @Composable ColumnScope.() -> Unit) {
    val shape = RoundedCornerShape(10.dp)
    Column(
        modifier
            .fillMaxWidth()
            .then(if (glow) Modifier.shadow(14.dp, shape, ambientColor = tone.color, spotColor = tone.color) else Modifier)
            .clip(shape)
            .background(Palette.surface)
            .background(tone.color.copy(alpha = 0.07f))
            .border(1.dp, tone.color.copy(alpha = 0.45f), shape)
            .drawBehind {
                val inset = 8.dp.toPx()
                drawRoundRect(tone.color, Offset(0f, inset), Size(3.dp.toPx(), size.height - 2 * inset), CornerRadius(3.dp.toPx()))
            }
            .padding(start = 14.dp, end = 12.dp, top = 10.dp, bottom = 10.dp),
        verticalArrangement = Arrangement.spacedBy(5.dp),
        content = content,
    )
}

/** A board's progress as one bar: done, review and doing in their lanes' tones. */
@Composable
fun LaneBar(done: Int, review: Int, doing: Int, total: Int, modifier: Modifier = Modifier) {
    val t = total.coerceAtLeast(1).toFloat()
    Row(modifier.fillMaxWidth().height(6.dp).clip(CircleShape).background(Palette.raised)) {
        for ((n, c) in listOf(done to Palette.ok, review to Palette.accent, doing to Palette.warn)) {
            if (n > 0) Box(Modifier.fillMaxHeight().weight(n / t).background(c))
        }
        val rest = (t - done - review - doing).coerceAtLeast(0f)
        if (rest > 0) Box(Modifier.weight(rest / t))
    }
}

/** An issue's or PR's state, as a small chip in its tone. */
@Composable
fun StateChip(state: String) {
    val tone = when (state) {
        "open" -> Tone.OK
        "merged" -> Tone.ACCENT
        "closed" -> Tone.MUTED
        else -> Tone.INFO
    }
    Text(
        state.uppercase(),
        Modifier.border(1.dp, tone.color.copy(alpha = 0.5f), CircleShape).padding(horizontal = 6.dp, vertical = 2.dp),
        style = Face.mono(9.sp, FontWeight.Bold).copy(letterSpacing = 0.6.sp),
        color = tone.color,
    )
}

/** A count in the display face, with its word in mono capitals. */
@Composable
fun Count(n: Int, label: String, tone: Tone, of: Int? = null) {
    Row(verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
        Text(of?.let { "$n/$it" } ?: "$n", style = Face.display(20.sp), color = tone.color)
        Caps(label, Modifier.padding(bottom = 3.dp))
    }
}

/** An answer's button: the accent filled for the first choice, an outline for the rest. */
@Composable
fun AnswerButton(text: String, modifier: Modifier = Modifier, primary: Boolean = false, danger: Boolean = false, enabled: Boolean = true, onClick: () -> Unit) {
    val pad = PaddingValues(horizontal = 12.dp, vertical = 6.dp)
    val shape = RoundedCornerShape(8.dp)
    if (primary) {
        Button(onClick, modifier.height(36.dp), enabled = enabled, shape = shape, contentPadding = pad,
            colors = ButtonDefaults.buttonColors(containerColor = Palette.accent, contentColor = Palette.void)) {
            Text(text, style = Face.ui(14.sp, FontWeight.SemiBold), maxLines = 1)
        }
    } else {
        OutlinedButton(onClick, modifier.height(36.dp), enabled = enabled, shape = shape, contentPadding = pad,
            border = androidx.compose.foundation.BorderStroke(1.dp, Palette.raised),
            colors = ButtonDefaults.outlinedButtonColors(containerColor = Palette.surface, contentColor = if (danger) Palette.danger else Palette.fg)) {
            Text(text, style = Face.ui(14.sp, FontWeight.SemiBold), maxLines = 1)
        }
    }
}

/** Someone's square: their initials on a color their name picks, the same every time. */
@Composable
fun Avatar(name: String, size: Int = 36, agent: Boolean = false) {
    val tones = listOf(Palette.accent, Palette.info, Palette.ok, Palette.peach, Palette.teal, Palette.pink, Palette.warn, Palette.merged)
    val words = name.split(' ', '-', '_', '.').filter { it.isNotEmpty() }
    val initials = words.take(2).map { it.first().uppercaseChar() }.joinToString("")
    var hash = 5381u
    for (c in name) hash = hash * 33u + c.code.toUInt()
    Box(Modifier.size(size.dp)) {
        Box(Modifier.size(size.dp).clip(RoundedCornerShape((size * 0.22).dp)).background(tones[(hash % tones.size.toUInt()).toInt()]), contentAlignment = Alignment.Center) {
            Text(initials.ifEmpty { "?" }, style = Face.display((size * 0.5).sp, FontWeight.Black), color = Palette.void)
        }
        if (agent) {
            Box(Modifier.align(Alignment.BottomEnd).size((size * 0.38).dp).clip(RoundedCornerShape(4.dp)).background(Palette.bg), contentAlignment = Alignment.Center) {
                Text("⚙", style = Face.ui((size * 0.26).sp), color = Palette.fg)
            }
        }
    }
}

/** What a list says while it loads. */
@Composable
fun Loading(modifier: Modifier = Modifier) {
    Box(modifier.fillMaxSize(), contentAlignment = Alignment.Center) { CircularProgressIndicator(color = Palette.accent) }
}

/** Says what went wrong, with a way to try again. */
@Composable
fun Problem(message: String, retry: (() -> Unit)? = null) {
    Column(Modifier.fillMaxSize().padding(24.dp), verticalArrangement = Arrangement.Center, horizontalAlignment = Alignment.CenterHorizontally) {
        Text("Can't load this", style = Face.ui(17.sp, FontWeight.SemiBold), color = Palette.danger)
        Text(message, Modifier.padding(top = 6.dp), style = Face.mono(12.sp), color = Palette.subtle)
        if (retry != null) TextButton(retry) { Text("Try again") }
    }
}
