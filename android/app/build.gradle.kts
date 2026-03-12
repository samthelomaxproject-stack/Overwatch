import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

val localProperties = Properties().apply {
    val f = rootProject.file("local.properties")
    if (f.exists()) load(f.inputStream())
}
val metaAppId = localProperties.getProperty("meta_app_id")
    ?: System.getenv("META_APP_ID")
    ?: "1460326209084504"
val metaClientToken = localProperties.getProperty("meta_client_token")
    ?: System.getenv("META_CLIENT_TOKEN")
    ?: "AR|1460326209084504|33a314a72066f4e4253315ae722fde16"

android {
    namespace = "ai.overwatch.android"
    compileSdk = 35

    defaultConfig {
        applicationId = "ai.overwatch.android"
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"

        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        manifestPlaceholders["META_APP_ID"] = metaAppId
        manifestPlaceholders["META_CLIENT_TOKEN"] = metaClientToken
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }
}

dependencies {
    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.appcompat:appcompat:1.7.0")
    implementation("com.google.android.material:material:1.12.0")
    implementation("androidx.constraintlayout:constraintlayout:2.1.4")

    implementation("com.meta.wearable:mwdat-core:0.4.0")
    implementation("com.meta.wearable:mwdat-camera:0.4.0")

    implementation("com.squareup.okhttp3:okhttp:4.12.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.8.1")

    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.2.1")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.6.1")
}
