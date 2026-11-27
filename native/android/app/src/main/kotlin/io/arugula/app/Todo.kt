// A to-do (Arugula #1470): work an agent hands its person because only they
// can do it (an approval on GitHub, a script with their credentials). It's a
// row on the Overview's Needs you and in the Inbox that expands in place: why
// the agent can't do it, how (its steps as written, in a mono well, with
// Copy), a note, and Done or Can't. The answer is `POST /api/inbox/{id}`
// with the choice and the note (InboxModel.choose, which an approval's row
// takes too); it wakes the agent that asked. The note goes in that request
// and nowhere else: it isn't kept here once it's sent.

package io.arugula.app

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/** An agent's to-do, answered Done or Can't. */
val InboxItem.isTodo get() = answer?.via == "todo"

/** Why the agent can't do it: its `Why: …` line, without the label. */
val InboxItem.why get() = lines.firstOrNull { it.startsWith("Why: ") }?.removePrefix("Why: ")

/** How: its other lines, its steps with their newlines, as written. */
val InboxItem.how get() = lines.filterNot { it.startsWith("Why: ") }.joinToString("\n").trim('\n').ifEmpty { null }

/**
 * A to-do expanded: its why, its how with Copy, a note, Done and Can't.
 * [done] says which was sent, and the row clears; a refusal (a bad choice,
 * someone else's or gone, already answered) shows the server's words.
 */
@Composable
fun TodoAnswer(i: InboxItem, inbox: InboxModel, done: (String) -> Unit) {
    val context = LocalContext.current
    val coroutines = rememberCoroutineScope()
    var note by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var problem by remember { mutableStateOf<String?>(null) }
    var copied by remember { mutableStateOf(false) }

    fun send(choice: String) {
        busy = true
        problem = null
        coroutines.launch {
            try {
                inbox.choose(i, choice, note)
                note = ""
                done(choice)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                problem = describe(e)
            }
            busy = false
        }
    }

    Column(Modifier.padding(top = 5.dp).alpha(if (busy) 0.5f else 1f).testTag("todo-${i.id}"), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        i.why?.let { Text(it, Modifier.testTag("todo-why"), style = Face.ui(14.sp), color = Palette.subtle) }
        i.how?.let { how ->
            val shape = RoundedCornerShape(8.dp)
            Column(Modifier.fillMaxWidth().background(Palette.surface, shape).border(1.dp, Palette.raised, shape).padding(10.dp).testTag("todo-how")) {
                SelectionContainer { Text(how, style = Face.mono(12.sp), color = Palette.fg) }
                Box(Modifier.fillMaxWidth().testTag("todo-copy"), contentAlignment = Alignment.CenterEnd) {
                    TextLink(if (copied) "Copied" else "Copy") {
                        (context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager).setPrimaryClip(ClipData.newPlainText("How", how))
                        copied = true
                        coroutines.launch {
                            delay(1500)
                            copied = false
                        }
                    }
                }
            }
        }
        OutlinedTextField(
            note, { note = it }, Modifier.fillMaxWidth().testTag("todo-note"),
            placeholder = { Text("A note (optional)") }, maxLines = 4, textStyle = Face.ui(14.sp), enabled = !busy,
            keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences, autoCorrectEnabled = false),
        )
        Text("Your agent reads the note: no passwords or keys.", style = Face.mono(11.sp), color = Palette.dim)
        FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            i.answer?.options.orEmpty().ifEmpty { listOf("Done", "Can't") }.forEachIndexed { n, c ->
                AnswerButton(c, Modifier.testTag("todo-$c"), primary = n == 0, enabled = !busy) { send(c) }
            }
        }
        problem?.let { Text(it, Modifier.testTag("todo-problem"), style = Face.mono(11.sp), color = Palette.danger, maxLines = 3) }
    }
}
