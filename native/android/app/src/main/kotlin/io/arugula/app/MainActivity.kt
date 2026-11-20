// The one activity: sign-in, or the app's tabs once signed in. It's
// singleTask, so the browser's Allow (`arugula://signin?ticket=…&grant=…`)
// comes back here, and the model the process keeps swaps it for a session.

package io.arugula.app

import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.repeatOnLifecycle
import io.arugula.core.CoreError
import kotlinx.coroutines.launch

class MainActivity : ComponentActivity() {
    private val model get() = App.model

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge(SystemBarStyle.dark(Palette.void.toArgb()), SystemBarStyle.dark(Palette.void.toArgb()))
        super.onCreate(savedInstanceState)
        // A test (debug builds only) points the app at its own control and
        // types the sign-in's answer by hand, as the iPhone's
        // ARUGULA_SIGNIN_BY_HAND does.
        if (BuildConfig.DEBUG) {
            intent.getStringExtra("control")?.let { SignInState.url = it }
            if (intent.getBooleanExtra("reset", false)) model.signOut()
            SignInState.byHand = intent.getBooleanExtra("signin_by_hand", false)
        }
        callback(intent)
        lifecycleScope.launch { repeatOnLifecycle(Lifecycle.State.RESUMED) { model.checkApp() } }
        setContent {
            ArugulaTheme {
                Root(model)
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        callback(intent)
    }

    /** The browser's Allow, sent to `arugula://signin`. */
    private fun callback(intent: Intent?) {
        val data = intent?.data ?: return
        if (data.scheme != "arugula" || data.host != "signin") return
        intent.data = null
        lifecycleScope.launch {
            SignInState.busy = true
            try {
                model.finishSignIn(data.toString())
                SignInState.problem = null
            } catch (e: Exception) {
                SignInState.problem = describe(e)
            } finally {
                SignInState.busy = false
            }
        }
    }
}

@Composable
private fun Root(model: AppModel) {
    val control = model.control
    Column(Modifier.fillMaxSize().background(Palette.void)) {
        when {
            model.updateNeeded -> UpdateApp(model)
            control != null -> Shell(control, model)
            else -> SignInScreen(model)
        }
    }
}

/** Control no longer serves this build of the app. */
@Composable
private fun UpdateApp(model: AppModel) {
    val scope = rememberCoroutineScope()
    Column(Modifier.fillMaxSize().padding(24.dp), verticalArrangement = Arrangement.spacedBy(12.dp, Alignment.CenterVertically), horizontalAlignment = Alignment.CenterHorizontally) {
        Text("UPDATE ARUGULA", style = Face.display(34.sp), color = Palette.fg)
        Text("Arugula cloud has moved on from this version of the app. Install the newest build to keep going.", style = Face.ui(15.sp), color = Palette.subtle)
        Button({ scope.launch { model.checkApp(force = true) } }) { Text("Try again") }
    }
}

/** Is it a refusal the sign-in screen says in its own words? */
fun signInWords(e: Throwable, model: AppModel, url: String): String? {
    val c = e as? CoreError ?: return describe(e)
    return when (c.reason) {
        "update_app" -> null.also { model.mustUpdate(url) }
        "older_control" -> "This Arugula isn't taking the Android app's sign-in yet. Its operator has to turn native clients on."
        else -> describe(e)
    }
}

@Composable
fun TextLink(text: String, onClick: () -> Unit) = TextButton(onClick) { Text(text, style = Face.mono(12.sp), color = Palette.accent) }
