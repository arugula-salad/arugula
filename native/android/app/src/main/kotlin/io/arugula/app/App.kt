// The app: sign in, then one navigation over projects, chat and machines,
// as the iPhone's App.swift. The model lives as long as the process, so a
// sign-in's verifier outlives the Custom Tab it waits on.

package io.arugula.app

import android.app.Application
import android.content.Context
import android.os.Build
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import io.arugula.core.Control
import io.arugula.core.Core
import io.arugula.core.CoreError
import io.arugula.core.SignIn
import java.io.File
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.MainScope
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject

class App : Application() {
    override fun onCreate() {
        super.onCreate()
        // Before anything reaches control: TLS checks its certificate against
        // Android's trust store through it.
        Core.init(this)
        model = AppModel(this)
    }

    companion object {
        lateinit var model: AppModel
            private set
    }
}

class AppModel(private val context: Context) {
    var control by mutableStateOf<Api?>(null)
        private set

    /** Why the app is asking for a sign-in again, if it's again. */
    var notice by mutableStateOf<String?>(null)

    /** Control no longer serves this build: the app says to update it. */
    var updateNeeded by mutableStateOf(false)
        private set

    /** A sign-in waiting on the browser's Allow, kept here and not in the activity. */
    var pending by mutableStateOf<SignIn?>(null)
        private set

    /** Where the session is, else where a sign-in would go. */
    var url = defaultUrl
        private set
    private var checked = 0L
    private val scope: CoroutineScope = MainScope()
    private val store = SessionStore(context)

    init {
        store.load()?.let { stored ->
            url = urlOf(stored) ?: url
            runCatching { open(stored) }.onFailure {
                store.forget()
                notice = "Sign in again: ${it.message}"
            }
        }
    }

    private fun open(stored: String) {
        val c = Control(stored)
        lateinit var api: Api
        api = Api(c, stored) {
            // Only the session in use: an old one's late answer ends nothing.
            scope.launch {
                if (control === api) {
                    store.forget()
                    forgetWebCookies()
                    control = null
                    notice = "Your sign-in has ended. Sign in again."
                }
            }
        }
        control = api
    }

    /** Ask control at [at] for a sign-in: its Allow page is [SignIn.link]. */
    suspend fun startSignIn(at: String): SignIn {
        val s = withContext(Dispatchers.IO) { SignIn.start(at, "Arugula on $device") }
        pending?.close()
        pending = s
        url = at
        return s
    }

    /** The browser's Allow came back to the app: swap its grant for a session. */
    suspend fun finishSignIn(callback: String) {
        val s = pending ?: throw IllegalStateException("No sign-in is waiting. Start again.")
        val stored = withContext(Dispatchers.IO) { s.finish(callback) }
        pending = null
        s.close()
        store.save(stored)
        url = urlOf(stored) ?: url
        notice = null
        open(stored)
    }

    fun cancelSignIn() {
        pending?.close()
        pending = null
    }

    /**
     * Ask control whether it still serves this build: as the app opens or
     * comes back, at most every ten minutes, or now with [force]. Offline
     * isn't an answer, so it leaves things as they are.
     */
    suspend fun checkApp(force: Boolean = false) {
        val now = System.currentTimeMillis()
        if (!force && !updateNeeded && now - checked < 600_000) return
        checked = now
        val e = withContext(Dispatchers.IO) { runCatching { Core.checkApp(url) }.exceptionOrNull() }
        if (e == null) updateNeeded = false else if ((e as? CoreError)?.updateApp == true) updateNeeded = true
    }

    /** A sign-in was refused because control needs a newer app. */
    fun mustUpdate(at: String) {
        url = at
        updateNeeded = true
    }

    fun signOut() {
        val c = control
        store.forget()
        forgetWebCookies()
        control = null
        if (c != null) scope.launch { withContext(Dispatchers.IO) { runCatching { c.control.signOut() } } }
    }

    companion object {
        /** Where control is. A test or a dev build can point elsewhere. */
        val defaultUrl: String get() = BuildConfig.CONTROL_URL

        val device: String get() = Build.MODEL ?: "Android"

        private fun urlOf(stored: String) = runCatching { JSONObject(stored).getString("url") }.getOrNull()
    }
}

/**
 * The sign-in: where, and the session control gave. Encrypted with an AES key
 * in the Android Keystore, in noBackupFilesDir: never backed up or moved to
 * another phone.
 */
class SessionStore(context: Context) {
    private val file = File(context.noBackupFilesDir, "session")

    private fun key(): SecretKey {
        val ks = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (ks.getKey(ALIAS, null) as? SecretKey)?.let { return it }
        val gen = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        gen.init(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build(),
        )
        return gen.generateKey()
    }

    fun load(): String? = runCatching {
        val bytes = file.readBytes()
        val iv = bytes.copyOfRange(0, 12)
        val c = Cipher.getInstance(CIPHER)
        c.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, iv))
        String(c.doFinal(bytes, 12, bytes.size - 12), Charsets.UTF_8)
    }.getOrNull()

    fun save(stored: String) {
        val c = Cipher.getInstance(CIPHER)
        c.init(Cipher.ENCRYPT_MODE, key())
        val tmp = File(file.path + ".tmp")
        tmp.writeBytes(c.iv + c.doFinal(stored.toByteArray(Charsets.UTF_8)))
        check(tmp.renameTo(file)) { "couldn't keep the sign-in" }
    }

    fun forget() {
        file.delete()
    }

    private companion object {
        const val ALIAS = "arugula-session"
        const val CIPHER = "AES/GCM/NoPadding"
    }
}
