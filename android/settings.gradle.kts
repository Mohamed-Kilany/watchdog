pluginManagement {
    // CI resilience: mirrors listed in WATCHDOG_GRADLE_MIRRORS (comma-separated)
    // are tried first. GitHub-hosted artifact redirects intermittently return
    // 504s; the mirrors serve the same Maven Central artifacts. Empty for
    // normal local builds, which use google()/mavenCentral()/Plugin Portal.
    val mirrors =
        (System.getenv("WATCHDOG_GRADLE_MIRRORS") ?: "")
            .split(",")
            .map { it.trim() }
            .filter { it.isNotEmpty() }
    repositories {
        mirrors.forEach { maven(it) }
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    val mirrors =
        (System.getenv("WATCHDOG_GRADLE_MIRRORS") ?: "")
            .split(",")
            .map { it.trim() }
            .filter { it.isNotEmpty() }
    repositories {
        mirrors.forEach { maven(it) }
        google()
        mavenCentral()
    }
}

rootProject.name = "watchdog-remote"
include(":app")
