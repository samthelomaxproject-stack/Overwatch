pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

import java.util.Properties

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
        val localProperties = Properties().apply {
            val f = File(rootDir, "local.properties")
            if (f.exists()) load(f.inputStream())
        }
        maven {
            url = uri("https://maven.pkg.github.com/facebook/meta-wearables-dat-android")
            credentials {
                username = System.getenv("GITHUB_ACTOR") ?: localProperties.getProperty("github_user") ?: "x-access-token"
                password = System.getenv("GITHUB_TOKEN") ?: localProperties.getProperty("github_token")
            }
        }
    }
}

rootProject.name = "OverwatchAndroid"
include(":app")
