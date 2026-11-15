import java.io.File

plugins { id("com.android.application"); kotlin("android") }

val debugKeystore = rootProject.layout.projectDirectory.file(".gradle-user/debug.keystore")
val prepareDebugKeystore by tasks.registering(Exec::class) {
    val destination = debugKeystore.asFile
    val keytool = File(System.getProperty("java.home"), "bin/" + if (System.getProperty("os.name").startsWith("Windows")) "keytool.exe" else "keytool")
    outputs.file(destination)
    onlyIf { !destination.exists() }
    doFirst { check(destination.parentFile.isDirectory || destination.parentFile.mkdirs()) }
    commandLine(keytool.absolutePath, "-genkeypair", "-keystore", destination.absolutePath,
        "-storepass", "android", "-keypass", "android", "-alias", "androiddebugkey",
        "-dname", "CN=Android Debug,O=Android,C=US", "-keyalg", "RSA", "-keysize", "2048", "-validity", "10000", "-noprompt")
}
tasks.matching { it.name == "validateSigningDebug" }.configureEach { dependsOn(prepareDebugKeystore) }

android {
    namespace = "com.siarheisinelnikau.wifimap"
    compileSdk = 35
    defaultConfig {
        applicationId = "com.siarheisinelnikau.wifimap"
        minSdk = 26
        targetSdk = 35
        versionCode = 2
        versionName = "0.2.0"
    }
    signingConfigs.getByName("debug") {
        storeFile = debugKeystore.asFile
    }
    buildTypes {
        debug { applicationIdSuffix = ".debug" }
        release { isMinifyEnabled = false }
    }
    buildFeatures { buildConfig = true }
    compileOptions { sourceCompatibility = JavaVersion.VERSION_17; targetCompatibility = JavaVersion.VERSION_17 }
    kotlinOptions { jvmTarget = "17" }
    packaging { resources.excludes += setOf("META-INF/DEPENDENCIES", "META-INF/INDEX.LIST") }
    testOptions { unitTests.isReturnDefaultValues = true }
}
dependencies {
    implementation(project(":domain"))
    implementation("androidx.activity:activity:1.10.1")
    implementation("androidx.lifecycle:lifecycle-viewmodel-ktx:2.6.2")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.6.2")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.10.2")
    implementation("androidx.annotation:annotation:1.9.1")
    testImplementation("junit:junit:4.13.2")
    constraints {
        implementation("androidx.annotation:annotation-experimental:1.4.1")
        implementation("androidx.collection:collection:1.1.0")
        implementation("androidx.concurrent:concurrent-futures:1.2.0")
        implementation("androidx.profileinstaller:profileinstaller:1.4.1")
        implementation("androidx.startup:startup-runtime:1.2.0")
    }
}
