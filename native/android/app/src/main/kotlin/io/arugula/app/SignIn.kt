// Signing in with the account (#999), as the iPhone does: control's page
// opens in a Custom Tab, where someone signed in to Arugula taps Allow, and
// the page sends the browser to `arugula://signin` with a one-time grant that
// only this app's waiting sign-in can redeem.

package io.arugula.app

import android.content.Context
import android.net.Uri
import androidx.browser.customtabs.CustomTabColorSchemeParams
import androidx.browser.customtabs.CustomTabsIntent
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
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
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.arugula.core.SignIn
import java.io.File
import kotlinx.coroutines.launch

/** The sign-in screen's state, kept across the trip to the browser and back. */
object SignInState {
    var url by mutableStateOf(AppModel.defaultUrl)
    var busy by mutableStateOf(false)
    var problem by mutableStateOf<String?>(null)

    /** A UI test can't tap Allow in a browser: it reads the ticket here and types the address the Allow was sent to. */
    var byHand by mutableStateOf(false)
}

@Composable
fun SignInScreen(model: AppModel) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var showAddress by remember { mutableStateOf(false) }
    val s = SignInState

    fun start() = scope.launch {
        s.busy = true
        s.problem = null
        try {
            val sign = model.startSignIn(s.url.trim())
            if (s.byHand) handOff(context, sign) else openTab(context, sign.link)
        } catch (e: Exception) {
            s.problem = signInWords(e, model, s.url)
        } finally {
            s.busy = false
        }
    }

    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(20.dp),
    ) {
        Column(Modifier.padding(top = 48.dp), horizontalAlignment = Alignment.CenterHorizontally) {
            Text("ARUGULA", style = Face.display(56.sp, FontWeight.Black), color = Palette.fg)
            Caps("Your machines, your agents, one place")
        }
        model.notice?.let {
            Text(it, Modifier.testTag("signin-notice"), style = Face.ui(13.sp), color = Palette.subtle, textAlign = TextAlign.Center)
        }
        val pending = model.pending
        Column(Modifier.widthIn(max = 520.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            if (pending != null && s.byHand) {
                byHand(model, pending)
            } else {
                Text(
                    "Sign in with your Arugula account. Arugula opens in a browser tab: sign in there if you aren't already, and tap Allow.",
                    style = Face.ui(15.sp), color = Palette.subtle, textAlign = TextAlign.Center,
                )
                Button(
                    { start() },
                    Modifier.fillMaxWidth().height(48.dp).testTag("signin-start"),
                    enabled = !s.busy,
                    colors = ButtonDefaults.buttonColors(containerColor = Palette.accent, contentColor = Palette.void),
                ) {
                    if (s.busy) {
                        CircularProgressIndicator(Modifier.size(18.dp), color = Palette.void, strokeWidth = 2.dp)
                        Text("  Waiting for Allow…", style = Face.ui(16.sp, FontWeight.SemiBold))
                    } else {
                        Text(if (pending != null) "Sign in again" else "Sign in", style = Face.ui(16.sp, FontWeight.SemiBold))
                    }
                }
                if (pending != null) {
                    Text("Waiting for Allow in the browser. Tap Sign in again to open it once more.", style = Face.ui(13.sp), color = Palette.dim, textAlign = TextAlign.Center)
                }
                TextLink(if (showAddress) "Hide address" else "Address") { showAddress = !showAddress }
                if (showAddress) {
                    OutlinedTextField(s.url, { s.url = it }, Modifier.fillMaxWidth().testTag("signin-url"), singleLine = true, label = { Text("Arugula's address") })
                }
            }
            s.problem?.let {
                Text(it, Modifier.fillMaxWidth().testTag("signin-problem"), style = Face.ui(13.sp), color = Palette.danger, textAlign = TextAlign.Center)
            }
        }
    }
}

/** The test's way in: the ticket to allow, and where to type the answer. */
@Composable
private fun byHand(model: AppModel, pending: SignIn) {
    val scope = rememberCoroutineScope()
    var callback by remember { mutableStateOf("") }
    SelectionContainer { Text(pending.ticket, Modifier.testTag("signin-ticket"), style = Face.mono(13.sp), color = Palette.fg) }
    OutlinedTextField(callback, { callback = it }, Modifier.fillMaxWidth().testTag("signin-callback"), singleLine = true, label = { Text("arugula://signin?…") })
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        AnswerButton("Finish", Modifier.testTag("signin-finish"), primary = true) {
            scope.launch {
                try {
                    model.finishSignIn(callback.trim())
                    SignInState.problem = null
                } catch (e: Exception) {
                    SignInState.problem = describe(e)
                }
            }
        }
        AnswerButton("Cancel") { model.cancelSignIn() }
    }
}

/** Control's Allow page in a Custom Tab, in the app's colors. */
private fun openTab(context: Context, link: String) {
    val colors = CustomTabColorSchemeParams.Builder().setToolbarColor(Palette.void.toArgb()).build()
    CustomTabsIntent.Builder()
        .setDefaultColorSchemeParams(colors)
        .setColorScheme(CustomTabsIntent.COLOR_SCHEME_DARK)
        .setShowTitle(true)
        .build()
        .launchUrl(context, Uri.parse(link))
}

/** The ticket where the test's driver reads it, as the core's FlowTest leaves it. */
private fun handOff(context: Context, s: SignIn) {
    val dir = File(context.filesDir, "handoff").apply { deleteRecursively(); mkdirs() }
    File(dir, "ticket").writeText(s.ticket)
}
