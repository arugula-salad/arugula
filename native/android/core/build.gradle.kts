// The core (native/core) as an Android library: its Kotlin side here, and
// the Rust built as a shared library for each ABI by cargo-ndk.
plugins {
    id("com.android.library")
}

android {
    namespace = "io.arugula.core"
    compileSdk = 36
    ndkVersion = "29.0.14206865"

    defaultConfig {
        // aws-lc and Ghostty build for API 26 and up.
        minSdk = 26
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        consumerProguardFiles("consumer-rules.pro")
    }

    sourceSets["main"].jniLibs.directories.add(layout.buildDirectory.dir("rustJniLibs").get().asFile.path)
}

// The same version as Cargo.lock's rustls-platform-verifier-android: the
// Rust half calls this Kotlin half by name.
val verifier = "0.2.0"

dependencies {
    implementation("org.rustls:rustls-platform-verifier:$verifier")
    androidTestImplementation("androidx.test:runner:1.7.0")
    androidTestImplementation("androidx.test.ext:junit:1.3.0")
    androidTestImplementation("junit:junit:4.13.2")
}

val coreDir = rootDir.resolve("../core")

val checkVerifier by tasks.registering {
    val lock = coreDir.resolve("Cargo.lock")
    inputs.file(lock)
    doLast {
        val lines = lock.readLines()
        val at = lines.indexOfFirst { it.trim() == "name = \"rustls-platform-verifier-android\"" }
        val locked = lines.getOrNull(at + 1)?.substringAfter('"')?.substringBefore('"')
        check(locked == verifier) { "Cargo.lock has rustls-platform-verifier-android $locked; core/build.gradle.kts says $verifier" }
    }
}

// libarugula_native_core.so for each ABI in arugula.abis, into rustJniLibs.
val cargoNdk by tasks.registering(Exec::class) {
    dependsOn(checkVerifier)
    val abis = providers.gradleProperty("arugula.abis").get().split(',').map { it.trim() }.filter { it.isNotEmpty() }
    val out = layout.buildDirectory.dir("rustJniLibs").get().asFile
    workingDir = coreDir
    inputs.dir(coreDir.resolve("src"))
    inputs.files(coreDir.resolve("Cargo.toml"), coreDir.resolve("Cargo.lock"))
    inputs.property("abis", abis)
    outputs.dir(out)
    val ndk = androidComponents.sdkComponents.ndkDirectory
    doFirst { environment("ANDROID_NDK_HOME", ndk.get().asFile.path) }
    commandLine(
        listOf("cargo", "ndk") + abis.flatMap { listOf("-t", it) } +
            listOf("-P", "26", "-o", out.path, "rustc", "--lib", "--release", "--features", "jni", "--crate-type", "cdylib"),
    )
}

tasks.named("preBuild") { dependsOn(cargoNdk) }
