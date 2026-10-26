import org.jetbrains.kotlin.gradle.dsl.JvmTarget
import java.io.File
import java.util.Properties

fun parseDotEnv(content: String): Map<String, String> {
    val out = mutableMapOf<String, String>()
    for (line in content.lines()) {
        val trimmed = line.trim()
        if (trimmed.isEmpty() || trimmed.startsWith("#")) continue
        val eq = trimmed.indexOf('=')
        if (eq <= 0) continue
        val key = trimmed.substring(0, eq).trim()
        var value = trimmed.substring(eq + 1).trim()
        if (
            (value.startsWith('"') && value.endsWith('"')) ||
            (value.startsWith('\'') && value.endsWith('\''))
        ) {
            value = value.substring(1, value.length - 1)
        }
        out[key] = value
    }
    return out
}

val monorepoEnvFile = layout.projectDirectory.file("../../../.env").asFile
val monorepoDotEnv: Map<String, String> =
    if (monorepoEnvFile.isFile) parseDotEnv(monorepoEnvFile.readText()) else emptyMap()

plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.serialization")
    id("maven-publish")
    id("signing")
}

group = "com.superglue.kt"
version = "1.0.0"

android {
    namespace = "com.superglue.kt"
    compileSdk = 36

    // Kept in sync with projects/Cargo.toml via scripts/sync-superglue-versions.sh
    @Suppress("UNUSED_VARIABLE")
    val superglueLibraryVersion = "1.0.0"

    defaultConfig {
        minSdk = 24
        consumerProguardFiles("consumer-rules.pro")

        ndk {
            abiFilters += listOf("arm64-v8a", "x86_64")
        }

        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlin {
        compilerOptions {
            jvmTarget.set(JvmTarget.JVM_17)
        }
    }

    sourceSets {
        getByName("main") {
            jniLibs.srcDir("src/main/jniLibs")
        }
    }

    testOptions {
        unitTests.all {
            val jniDir = layout.projectDirectory.dir("src/main/jniLibs/x86_64").asFile
            if (jniDir.isDirectory) {
                val path = jniDir.absolutePath
                it.jvmArgs("-Djava.library.path=$path")
                it.environment("LD_LIBRARY_PATH", path)
            }
            // Mirror scripts/run-all-superglue-examples.sh: fill OPENAI_* from repo .env when unset.
            listOf("OPENAI_API_KEY", "OPENAI_MODEL", "OPENAI_BASE_URL").forEach { key ->
                if (System.getenv(key).isNullOrBlank()) {
                    monorepoDotEnv[key]?.takeIf { it.isNotBlank() }?.let { value ->
                        it.environment(key, value)
                    }
                }
            }
        }
    }

    publishing {
        singleVariant("release") {
            withSourcesJar()
        }
    }
}

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.kotlinx.coroutines.android)
    implementation(libs.kotlinx.serialization.json)

    testImplementation(libs.junit)
    testImplementation(libs.kotlinx.coroutines.test)
}

// ---------------------------------------------------------------------------
// NDK: prefer Gradle `android.ndkPath`, then env, then `sdk.dir` from
// `local.properties` (typically at the app root) + `ndk/<version>`.
// ---------------------------------------------------------------------------

fun org.gradle.api.Project.resolveAndroidNdkPath(): String? {
    val fromGradle = findProperty("android.ndkPath") as? String
    if (!fromGradle.isNullOrBlank()) {
        return fromGradle.trim()
    }
    listOf("ANDROID_NDK_HOME", "ANDROID_NDK_ROOT").forEach { key ->
        val v = System.getenv(key)
        if (!v.isNullOrBlank()) {
            return v.trim()
        }
    }
    // Root project is Butler when included; may also be superglue-kt when opened alone.
    val localProps = rootProject.file("local.properties")
    if (!localProps.exists()) {
        return null
    }
    val p = Properties()
    localProps.inputStream().use { p.load(it) }
    val sdkDir = p.getProperty("sdk.dir")?.trim() ?: return null
    val ndkBase = file(sdkDir).resolve("ndk")
    if (!ndkBase.isDirectory) {
        return null
    }
    val installed = ndkBase.listFiles()?.filter { it.isDirectory }.orEmpty()
    if (installed.isEmpty()) {
        return null
    }
    // Prefer highest NDK version (first dotted segment is major, e.g. 26 > 25 > 9)
    return installed.maxWithOrNull(
        compareBy<File>(
            { it.name.split(".").firstOrNull()?.toIntOrNull() ?: 0 },
            { it.name },
        ),
    )?.absolutePath
}

// Build Rust + JNI and copy .so into jniLibs/ (requires: Rust, cargo-ndk, Android NDK).
// `native/` is ../native from this module (i.e. superglue-kt/native).
val nativeDir = project.layout.projectDirectory.dir("../native").asFile
val jniOutDir = layout.projectDirectory.dir("src/main/jniLibs").asFile
val abis = listOf("arm64-v8a", "x86_64")
/** `superglue` path dependency of `../native` (../../superglue from native crate). */
val superglueDir = nativeDir.parentFile.parentFile.resolve("superglue")

val resolvedNdkPath: String? = project.resolveAndroidNdkPath()

// Force the host `cargo build` path even when an NDK is on the machine
// (GitHub runners often set ANDROID_NDK_HOME). Use for CI unit tests.
val forceHostJni =
    (findProperty("superglue.forceHostJni") as? String)?.equals("true", ignoreCase = true) == true ||
        System.getenv("SUPERGLUE_FORCE_HOST_JNI") == "1"

val canBuildRustJni: Boolean =
    !forceHostJni && resolvedNdkPath != null && nativeDir.resolve("Cargo.toml").exists()

val skipNativeBuild =
    (findProperty("superglue.skipNativeBuild") as? String)?.equals("true", ignoreCase = true) == true ||
        System.getenv("SUPERGLUE_SKIP_NATIVE_BUILD") == "1"
val shouldWireRustJni: Boolean = canBuildRustJni && !skipNativeBuild
for (abi in abis) {
    File(jniOutDir, abi).mkdirs()
}

val buildRustJni: org.gradle.api.tasks.TaskProvider<org.gradle.api.tasks.Exec> =
    tasks.register<org.gradle.api.tasks.Exec>("buildRustJni") {
        group = "build"
        description =
            "Build libsuperglue_kt.so for Android ABIs via cargo-ndk. " +
                "Skips with -Psuperglue.skipNativeBuild=true or SUPERGLUE_SKIP_NATIVE_BUILD=1 (reuse jniLibs). " +
                "Stays up-to-date when listed inputs are older than the .so files."
        onlyIf { shouldWireRustJni }

        inputs.file(nativeDir.resolve("Cargo.toml"))
        if (nativeDir.resolve("Cargo.lock").isFile) {
            inputs.file(nativeDir.resolve("Cargo.lock"))
        }
        inputs.dir(nativeDir.resolve("src"))
        if (nativeDir.resolve("build.rs").isFile) {
            inputs.file(nativeDir.resolve("build.rs"))
        }
        if (superglueDir.isDirectory) {
            val sgCargo = superglueDir.resolve("Cargo.toml")
            if (sgCargo.isFile) {
                inputs.file(sgCargo)
            }
            val sgSrc = superglueDir.resolve("src")
            if (sgSrc.isDirectory) {
                inputs.dir(sgSrc)
            }
        }
        for (abi in abis) {
            outputs.file(File(jniOutDir, "$abi/libsuperglue_kt.so"))
        }

        doFirst {
            val ndk = resolvedNdkPath
                ?: throw GradleException("NDK not resolved; set android.ndkPath, ANDROID_NDK_HOME, or sdk.dir+ndk/ in local.properties")
            environment("ANDROID_NDK_HOME", ndk)
            environment("ANDROID_NDK_ROOT", ndk)
        }

        workingDir = nativeDir
        commandLine = buildList {
            add("cargo")
            add("ndk")
            add("-o")
            add(jniOutDir.absolutePath)
            abis.forEach { add("-t"); add(it) }
            add("build")
            add("--release")
        }
    }

val workspaceTargetDir = nativeDir.parentFile.parentFile.resolve("target")

val buildRustJniHost: org.gradle.api.tasks.TaskProvider<org.gradle.api.tasks.Exec> =
    tasks.register<org.gradle.api.tasks.Exec>("buildRustJniHost") {
        group = "build"
        description =
            "Build libsuperglue_kt.so for host unit tests (x86_64) via cargo when the Android NDK is unavailable."
        onlyIf { !shouldWireRustJni && !skipNativeBuild && nativeDir.resolve("Cargo.toml").exists() }

        inputs.file(nativeDir.resolve("Cargo.toml"))
        if (nativeDir.resolve("Cargo.lock").isFile) {
            inputs.file(nativeDir.resolve("Cargo.lock"))
        }
        inputs.dir(nativeDir.resolve("src"))
        if (nativeDir.resolve("build.rs").isFile) {
            inputs.file(nativeDir.resolve("build.rs"))
        }
        if (superglueDir.isDirectory) {
            val sgCargo = superglueDir.resolve("Cargo.toml")
            if (sgCargo.isFile) {
                inputs.file(sgCargo)
            }
            val sgSrc = superglueDir.resolve("src")
            if (sgSrc.isDirectory) {
                inputs.dir(sgSrc)
            }
        }
        outputs.file(File(jniOutDir, "x86_64/libsuperglue_kt.so"))

        workingDir = nativeDir
        // Pin the target dir so the copy step matches cargo output even when
        // CARGO_TARGET_DIR is set in the environment.
        commandLine(
            "cargo",
            "build",
            "--release",
            "--target-dir",
            workspaceTargetDir.absolutePath,
        )

        doLast {
            val built = workspaceTargetDir.resolve("release/libsuperglue_kt.so")
            if (!built.isFile) {
                throw GradleException("Expected ${built.absolutePath} after cargo build")
            }
            built.copyTo(File(jniOutDir, "x86_64/libsuperglue_kt.so"), overwrite = true)
        }
    }

afterEvaluate {
    val testTask = tasks.findByName("testDebugUnitTest")
    if (shouldWireRustJni) {
        listOf(
            "preBuild",
            "mergeDebugJniLibFolders",
            "mergeReleaseJniLibFolders",
            "testDebugUnitTest",
        ).forEach { taskName ->
            tasks.findByName(taskName)?.dependsOn(buildRustJni)
        }
    } else if (!skipNativeBuild) {
        testTask?.dependsOn(buildRustJniHost)
    }
}

// Maven Central requires a javadoc jar. The sources jar holds the API.
val centralJavadocJar = tasks.register<org.gradle.api.tasks.bundling.Jar>("centralJavadocJar") {
    group = "publishing"
    description = "Javadoc stub for Maven Central. Read the sources jar for the API."
    archiveClassifier.set("javadoc")
    val stubDir = layout.buildDirectory.dir("javadoc-stub")
    doFirst {
        val dir = stubDir.get().asFile
        dir.mkdirs()
        dir.resolve("README.txt").writeText(
            "Kotlin and Java sources are in the sources jar.\n",
        )
    }
    from(stubDir)
}

publishing {
    repositories {
        maven {
            name = "local"
            url = uri(layout.buildDirectory.dir("repo"))
        }
    }
}

afterEvaluate {
    publishing.publications.create<org.gradle.api.publish.maven.MavenPublication>("release") {
        from(components["release"])
        groupId = "com.superglue.kt"
        artifactId = "superglue-android"
        version = project.version.toString()
        artifact(centralJavadocJar)
        pom {
            name.set("superglue-android")
            description.set("Android JNI bindings for the superglue Rust core")
            url.set("https://github.com/pinhole-io/superglue")
            licenses {
                license {
                    name.set("MIT")
                    url.set("https://opensource.org/licenses/MIT")
                }
            }
            developers {
                developer {
                    name.set("Nick")
                    email.set("nick@nyoung.dev")
                }
            }
            scm {
                connection.set("scm:git:https://github.com/pinhole-io/superglue.git")
                developerConnection.set("scm:git:ssh://git@github.com/pinhole-io/superglue.git")
                url.set("https://github.com/pinhole-io/superglue")
            }
        }
    }

    val signingKey = System.getenv("GPG_PRIVATE_KEY")
    if (!signingKey.isNullOrBlank()) {
        signing {
            useInMemoryPgpKeys(signingKey, System.getenv("GPG_PASSPHRASE").orEmpty())
            sign(publishing.publications.getByName("release"))
        }
    }
}
