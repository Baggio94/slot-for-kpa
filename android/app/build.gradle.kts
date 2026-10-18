plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "fyi.slot.konkr"
    compileSdk = 35

    defaultConfig {
        applicationId = "fyi.slot.konkr"
        minSdk = 31
        targetSdk = 32
        // Dedicated KONKR ARM64 build; retain the stable package and signing
        // identity so adb install -r preserves existing data and SAF grants.
        // Verified KONKR 1.2.1 stable upgrades 1.2.0 (114) and betas 115-119.
        versionCode = 120
        versionName = "1.2.1"
        ndk { abiFilters += listOf("arm64-v8a") }
    }

    // This APK is distributed directly for KONKR Android 12 (not via Google
    // Play). Keep targetSdk 32 to preserve the validated system/input behavior;
    // suppress only Play's expired-target submission check, not other lint.
    lint {
        disable += "ExpiredTargetSdkVersion"
    }

    // A stable signing key can be provided by GitHub Actions secrets or a
    // local development environment. Never commit the keystore or credentials.
    // Without these variables Gradle falls back to its ephemeral debug key.
    val persistentStore = System.getenv("SLOT_SIGNING_STORE_FILE")
    if (!persistentStore.isNullOrBlank()) {
        signingConfigs {
            create("slotPersistent") {
                storeFile = file(persistentStore)
                storePassword = System.getenv("SLOT_SIGNING_STORE_PASSWORD")
                    ?: error("SLOT_SIGNING_STORE_PASSWORD is required")
                keyAlias = System.getenv("SLOT_SIGNING_KEY_ALIAS")
                    ?: error("SLOT_SIGNING_KEY_ALIAS is required")
                keyPassword = System.getenv("SLOT_SIGNING_KEY_PASSWORD")
                    ?: error("SLOT_SIGNING_KEY_PASSWORD is required")
            }
        }
        buildTypes {
            getByName("debug") {
                signingConfig = signingConfigs.getByName("slotPersistent")
            }
            getByName("release") {
                // Release candidates must be non-debuggable while preserving
                // the same update-compatible signing certificate as debug RCs.
                isDebuggable = false
                signingConfig = signingConfigs.getByName("slotPersistent")
            }
        }
    }

    // Reuse the exact upstream Slot insert/eject PCM assets.
    sourceSets.getByName("main").assets.srcDir("../../crates/slot/assets")
    // Original Slot label font (SIL OFL) for a visually consistent account panel.
    // Include the accompanying OFL license from the same upstream source directory.
    sourceSets.getByName("main").assets.srcDir("../../crates/slot-ui/assets")

    packaging {
        jniLibs { useLegacyPackaging = true }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
}

dependencies {
    implementation("androidx.core:core:1.13.1")
    testImplementation("junit:junit:4.13.2")
}
