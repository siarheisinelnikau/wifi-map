plugins { kotlin("jvm") }
kotlin {
    jvmToolchain(21)
    compilerOptions.jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
}
dependencies {
    implementation("com.google.code.gson:gson:2.11.0")
    implementation("org.jetbrains:annotations:23.0.0")
    testImplementation("junit:junit:4.13.2")
}
sourceSets.test { resources.srcDir("../../shared/fixtures") }
tasks.test {
    maxParallelForks = 1
    systemProperty("interop.output", layout.buildDirectory.file("interop/android-roundtrip.wifimap").get().asFile.absolutePath)
}
