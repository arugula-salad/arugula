import java.security.MessageDigest

// The Android app (#1249): Jetpack Compose screens over the core (`:core`),
// doing what the iPhone app does.
plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.plugin.compose")
}

android {
    namespace = "io.arugula.app"
    // Compose and AndroidX ask for 37; the app runs from 26 on.
    compileSdk = 37
    compileSdkMinor = 2

    defaultConfig {
        applicationId = "io.arugula.app"
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        // Where control is; a test or a dev build points elsewhere with
        // -Parugula.control=URL.
        buildConfigField(
            "String",
            "CONTROL_URL",
            "\"" + (providers.gradleProperty("arugula.control").orNull ?: "https://control.arugula.io") + "\"",
        )
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    sourceSets["main"].res.directories.add(layout.buildDirectory.dir("generated/fonts").get().asFile.path)
}

dependencies {
    implementation(project(":core"))
    implementation(platform("androidx.compose:compose-bom:2026.11.01"))
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    // Machines' list beside its session on a tablet (ListDetailPaneScaffold).
    implementation("androidx.compose.material3.adaptive:adaptive:1.3.0")
    implementation("androidx.compose.material3.adaptive:adaptive-layout:1.3.0")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.activity:activity-compose:1.13.0")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.11.1")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.11.1")
    implementation("androidx.browser:browser:1.10.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.10.2")
    debugImplementation("androidx.compose.ui:ui-tooling")

    androidTestImplementation(platform("androidx.compose:compose-bom:2026.11.01"))
    androidTestImplementation("androidx.compose.ui:ui-test-junit4")
    debugImplementation("androidx.compose.ui:ui-test-manifest")
    androidTestImplementation("androidx.test:runner:1.7.0")
    androidTestImplementation("androidx.test:core:1.7.0")
    androidTestImplementation("androidx.test.ext:junit:1.3.0")
    androidTestImplementation("junit:junit:4.13.2")
}

// The swarm's fonts, as the web sets them and the iPhone's build.sh fetches
// them: Big Shoulders Display, Instrument Sans and JetBrains Mono (SIL Open
// Font License 1.1), from Google Fonts at a pinned commit and checked. The
// repository keeps no binaries, so they're fetched into the build.
val fonts = tasks.register("fonts") {
    val rev = "c30b221beb0c1da59d563feb91ec8656a1fa972a"
    val list = listOf(
        Triple("60e208dc276a1c35fc5b62e94f9fb959c40c11783a9eb7548175c14b1fbeb720", "bigshouldersdisplay/BigShouldersDisplay%5Bwght%5D.ttf", "big_shoulders.ttf"),
        Triple("b24f1812584816958afcf22e22d08e44318c5e51651e25d2438efdde389b33b1", "instrumentsans/InstrumentSans%5Bwdth,wght%5D.ttf", "instrument_sans.ttf"),
        Triple("48715a42ec242c21e9f02692891e147d022299a52e48d5e413e1a942193ffeda", "jetbrainsmono/JetBrainsMono%5Bwght%5D.ttf", "jetbrains_mono.ttf"),
    )
    val dir = layout.buildDirectory.dir("generated/fonts/font").get().asFile
    inputs.property("fonts", list.toString())
    outputs.dir(dir)
    doLast {
        dir.mkdirs()
        fun sha(f: File) = MessageDigest.getInstance("SHA-256").digest(f.readBytes()).joinToString("") { "%02x".format(it) }
        for ((sum, path, name) in list) {
            val file = dir.resolve(name)
            if (file.exists() && sha(file) == sum) continue
            val part = dir.resolve("$name.part")
            uri("https://raw.githubusercontent.com/google/fonts/$rev/ofl/$path").toURL().openStream().use { i -> part.outputStream().use { i.copyTo(it) } }
            check(sha(part) == sum) { "$name isn't the font it should be" }
            part.renameTo(file)
        }
    }
}

tasks.named("preBuild") { dependsOn(fonts) }
