import java.io.ByteArrayOutputStream
import java.io.File

plugins {
    id("com.android.application")
    kotlin("android")
}

android {
    namespace = "com.benfach.cliplink"
    compileSdk = 34

    defaultConfig {
        applicationId = "com.benfach.cliplink"
        minSdk = 29
        targetSdk = 34
        versionCode = 1
        versionName = "0.1.0"

        ndk {
            abiFilters += listOf("arm64-v8a")
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
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

    packaging {
        jniLibs.useLegacyPackaging = false
    }

    sourceSets["main"].jniLibs.srcDir(layout.buildDirectory.dir("rustJniLibs"))
}

val rustWorkspaceDir = rootDir.parentFile
val rustOutputDir = layout.buildDirectory.dir("rustJniLibs")

fun commandOutput(vararg command: String): String? {
    val stdout = ByteArrayOutputStream()
    val stderr = ByteArrayOutputStream()
    val result = runCatching {
        project.exec {
            commandLine(*command)
            standardOutput = stdout
            errorOutput = stderr
            isIgnoreExitValue = true
        }
    }.getOrNull() ?: return null
    if (result.exitValue != 0) {
        return null
    }
    return stdout.toString().trim().takeIf { it.isNotEmpty() }
}

val rustupToolchain = providers.environmentVariable("CLIPLINK_RUSTUP_TOOLCHAIN")
    .orNull
    ?.trim()
    ?.takeIf { it.isNotEmpty() }
    ?: commandOutput("rustup", "show", "active-toolchain")?.substringBefore(' ')

val rustupCargo = rustupToolchain?.let { toolchain ->
    commandOutput("rustup", "which", "--toolchain", toolchain, "cargo")
}
val rustupRustc = rustupToolchain?.let { toolchain ->
    commandOutput("rustup", "which", "--toolchain", toolchain, "rustc")
}
val rustToolchainBinDir = rustupCargo?.let { cargoPath ->
    File(cargoPath).parent
}

val rustBuildProfile = providers.provider {
    if (gradle.startParameter.taskNames.any { it.contains("Release", ignoreCase = true) }) {
        "release"
    } else {
        "debug"
    }
}

fun cargoCommand(vararg extraArgs: String): List<String> {
    val prefix = if (rustupCargo != null) {
        listOf(rustupCargo)
    } else {
        listOf("cargo")
    }
    return prefix + extraArgs
}

fun org.gradle.process.ExecSpec.applyRustToolchainEnvironment() {
    rustupRustc?.let { rustcPath ->
        environment("RUSTC", rustcPath)
    }
    rustToolchainBinDir?.let { toolchainBinDir ->
        val currentPath = System.getenv("PATH").orEmpty()
        val mergedPath = if (currentPath.isBlank()) {
            toolchainBinDir
        } else {
            "$toolchainBinDir${File.pathSeparator}$currentPath"
        }
        environment("PATH", mergedPath)
    }
}

val verifyCargoNdk by tasks.registering {
    doLast {
        val output = ByteArrayOutputStream()
        project.exec {
            applyRustToolchainEnvironment()
            commandLine(cargoCommand("ndk", "--version"))
            standardOutput = output
            errorOutput = output
            isIgnoreExitValue = true
        }.assertNormalExitValue()
    }
}

val buildRustJniLibs by tasks.registering(Exec::class) {
    dependsOn(verifyCargoNdk)
    workingDir = rustWorkspaceDir
    val outputDir = rustOutputDir.get().asFile.absolutePath
    val args = cargoCommand(
        "ndk",
        "-t",
        "arm64-v8a",
        "-o",
        outputDir,
        "build",
        "--lib",
    )
    applyRustToolchainEnvironment()
    if (rustBuildProfile.get() == "release") {
        commandLine(args + "--release")
    } else {
        commandLine(args)
    }
}

tasks.matching { task ->
    task.name.startsWith("merge") && task.name.endsWith("JniLibFolders")
}.configureEach {
    dependsOn(buildRustJniLibs)
}

dependencies {
    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.appcompat:appcompat:1.7.0")
    implementation("androidx.recyclerview:recyclerview:1.3.2")
}
