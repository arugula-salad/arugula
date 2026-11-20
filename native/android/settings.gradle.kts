// Android's Arugula: the core's Kotlin side (`core`), and the app on it (#1249).
pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
        // rustls-platform-verifier's Kotlin half, which its Rust half calls to
        // check certificates against Android's trust store.
        maven("https://github.com/rustls/rustls-platform-verifier/raw/maven-archive/android-release-support/maven/") {
            content { includeGroup("org.rustls") }
        }
    }
}

rootProject.name = "arugula-android"
include(":core")
include(":app")
