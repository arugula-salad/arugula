plugins {
    id("com.android.library") version "9.5.1" apply false
    id("com.android.application") version "9.5.1" apply false
    // AGP 9 has Kotlin built in; Compose's compiler plugin is the one extra,
    // at the Kotlin it brings.
    id("org.jetbrains.kotlin.plugin.compose") version "2.4.21" apply false
}
