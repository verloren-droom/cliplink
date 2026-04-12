import java.io.ByteArrayOutputStream
import java.io.File
import java.util.Locale
import javax.xml.parsers.DocumentBuilderFactory
import org.w3c.dom.Element

plugins {
    id("com.android.application")
    kotlin("android")
}

val androidReleaseKeystorePath = providers.environmentVariable("ANDROID_KEYSTORE_PATH")
    .orNull
    ?.trim()
    ?.takeIf { it.isNotEmpty() }
val androidReleaseStorePassword = providers.environmentVariable("ANDROID_KEYSTORE_PASSWORD")
    .orNull
    ?.trim()
    ?.takeIf { it.isNotEmpty() }
val androidReleaseKeyAlias = providers.environmentVariable("ANDROID_KEY_ALIAS")
    .orNull
    ?.trim()
    ?.takeIf { it.isNotEmpty() }
val androidReleaseKeyPassword = providers.environmentVariable("ANDROID_KEY_PASSWORD")
    .orNull
    ?.trim()
    ?.takeIf { it.isNotEmpty() }
val androidReleaseSigningReady = listOf(
    androidReleaseKeystorePath,
    androidReleaseStorePassword,
    androidReleaseKeyAlias,
    androidReleaseKeyPassword,
).all { it != null }

android {
    namespace = "com.benfach.cliplink"
    compileSdk = 34
    ndkVersion = "26.3.11579264"

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

    signingConfigs {
        create("release") {
            if (androidReleaseSigningReady) {
                storeFile = file(androidReleaseKeystorePath!!)
                storePassword = androidReleaseStorePassword
                keyAlias = androidReleaseKeyAlias
                keyPassword = androidReleaseKeyPassword
                enableV1Signing = true
                enableV2Signing = true
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
            )
            if (androidReleaseSigningReady) {
                signingConfig = signingConfigs.getByName("release")
            }
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
val generatedLauncherIconsResDir = layout.buildDirectory.dir("generated/res/launcherIcons")

android.sourceSets["main"].res.srcDir(generatedLauncherIconsResDir)

data class SvgPathSpec(
    val pathData: String,
    val fillColor: String?,
    val strokeColor: String?,
    val strokeWidth: String?,
    val strokeLineCap: String?,
    val strokeLineJoin: String?,
)

data class SvgViewportSpec(
    val width: String,
    val height: String,
)

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

fun normalizeSvgPaint(value: String?): String? {
    val normalized = value?.trim().orEmpty()
    if (normalized.isEmpty() || normalized.equals("none", ignoreCase = true)) {
        return null
    }
    return when {
        normalized.startsWith("#") -> normalized.uppercase(Locale.ROOT)
        normalized.equals("black", ignoreCase = true) -> "#000000"
        normalized.equals("white", ignoreCase = true) -> "#FFFFFF"
        else -> error("Unsupported SVG paint value: $normalized")
    }
}

fun parseSvgViewport(svgRoot: Element): SvgViewportSpec {
    val viewBoxParts = svgRoot.getAttribute("viewBox").trim().split(Regex("\\s+"))
    require(viewBoxParts.size == 4) {
        "resources/icon.svg must define a four-value viewBox"
    }
    return SvgViewportSpec(
        width = viewBoxParts[2],
        height = viewBoxParts[3],
    )
}

fun parseSvgPaths(svgFile: File): Pair<SvgViewportSpec, List<SvgPathSpec>> {
    val documentBuilderFactory = DocumentBuilderFactory.newInstance().apply {
        isNamespaceAware = false
    }
    val document = documentBuilderFactory.newDocumentBuilder().parse(svgFile)
    val svgRoot = document.documentElement
    val viewport = parseSvgViewport(svgRoot)
    val pathNodes = svgRoot.getElementsByTagName("path")
    val paths = buildList(pathNodes.length) {
        for (index in 0 until pathNodes.length) {
            val node = pathNodes.item(index)
            if (node !is Element) continue
            add(
                SvgPathSpec(
                    pathData = node.getAttribute("d").trim().ifEmpty {
                        error("SVG path is missing d attribute")
                    },
                    fillColor = normalizeSvgPaint(node.getAttribute("fill")),
                    strokeColor = normalizeSvgPaint(node.getAttribute("stroke")),
                    strokeWidth = node.getAttribute("stroke-width").trim().ifEmpty { null },
                    strokeLineCap = node.getAttribute("stroke-linecap").trim().ifEmpty { null },
                    strokeLineJoin = node.getAttribute("stroke-linejoin").trim().ifEmpty { null },
                )
            )
        }
    }
    require(paths.isNotEmpty()) {
        "resources/icon.svg must contain at least one path"
    }
    return viewport to paths
}

fun androidPaintValue(original: String?, overrideColor: String? = null): String {
    return overrideColor ?: original ?: "#00000000"
}

fun svgPathToVectorXml(path: SvgPathSpec, monochrome: Boolean): String {
    val colorOverride = if (monochrome) "#111111" else null
    return buildString {
        appendLine("    <path")
        appendLine("        android:fillColor=\"${androidPaintValue(path.fillColor, if (path.fillColor != null) colorOverride else null)}\"")
        appendLine("        android:pathData=\"${path.pathData}\"")
        if (path.strokeColor != null) {
            appendLine("        android:strokeColor=\"${androidPaintValue(path.strokeColor, colorOverride)}\"")
        }
        path.strokeLineCap?.let { appendLine("        android:strokeLineCap=\"$it\"") }
        path.strokeLineJoin?.let { appendLine("        android:strokeLineJoin=\"$it\"") }
        path.strokeWidth?.let { appendLine("        android:strokeWidth=\"$it\"") }
        appendLine(" />")
    }
}

fun buildLauncherVectorDrawableXml(
    viewport: SvgViewportSpec,
    paths: List<SvgPathSpec>,
    monochrome: Boolean,
): String {
    return buildString {
        appendLine("<?xml version=\"1.0\" encoding=\"utf-8\"?>")
        appendLine("<vector xmlns:android=\"http://schemas.android.com/apk/res/android\"")
        appendLine("    android:width=\"108dp\"")
        appendLine("    android:height=\"108dp\"")
        appendLine("    android:viewportWidth=\"${viewport.width}\"")
        appendLine("    android:viewportHeight=\"${viewport.height}\">")
        for (path in paths) {
            append(svgPathToVectorXml(path, monochrome))
        }
        appendLine("</vector>")
    }
}

fun buildAdaptiveIconXml(): String {
    return """
        <?xml version="1.0" encoding="utf-8"?>
        <adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">
            <background android:drawable="@color/color_launcher_background" />
            <foreground android:drawable="@drawable/ic_launcher_foreground" />
            <monochrome android:drawable="@drawable/ic_launcher_monochrome" />
        </adaptive-icon>
    """.trimIndent() + "\n"
}

fun writeTextFile(file: File, content: String) {
    file.parentFile.mkdirs()
    file.writeText(content)
}

fun generateLauncherIconResources(svgFile: File, outputDir: File) {
    val (viewport, paths) = parseSvgPaths(svgFile)

    outputDir.deleteRecursively()
    outputDir.mkdirs()

    writeTextFile(
        outputDir.resolve("drawable/ic_launcher_foreground.xml"),
        buildLauncherVectorDrawableXml(viewport, paths, monochrome = false),
    )
    writeTextFile(
        outputDir.resolve("drawable/ic_launcher_monochrome.xml"),
        buildLauncherVectorDrawableXml(viewport, paths, monochrome = true),
    )
    writeTextFile(
        outputDir.resolve("mipmap-anydpi-v26/ic_launcher.xml"),
        buildAdaptiveIconXml(),
    )
    writeTextFile(
        outputDir.resolve("mipmap-anydpi-v26/ic_launcher_round.xml"),
        buildAdaptiveIconXml(),
    )
}

val generateLauncherIcons by tasks.registering {
    val sourceIconFile = rustWorkspaceDir.resolve("resources/icon.svg")

    inputs.file(sourceIconFile)
    outputs.dir(generatedLauncherIconsResDir)
    outputs.upToDateWhen { false }

    doLast {
        require(sourceIconFile.isFile) {
            "Missing launcher icon source: ${sourceIconFile.absolutePath}"
        }
        generateLauncherIconResources(sourceIconFile, generatedLauncherIconsResDir.get().asFile)
    }
}

tasks.named("preBuild").configure {
    dependsOn(generateLauncherIcons)
}

val rustupToolchain = providers.environmentVariable("RUSTUP_TOOLCHAIN")
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
