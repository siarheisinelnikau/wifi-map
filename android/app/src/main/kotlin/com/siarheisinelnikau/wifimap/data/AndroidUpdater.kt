package com.siarheisinelnikau.wifimap.data

import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageInfo
import android.content.pm.PackageInstaller
import android.content.pm.PackageManager
import android.os.Build
import com.siarheisinelnikau.wifimap.BuildConfig
import com.siarheisinelnikau.wifimap.domain.*
import com.siarheisinelnikau.wifimap.platform.InstallResultReceiver
import kotlinx.coroutines.*
import java.io.File
import java.net.URL
import java.security.MessageDigest
import javax.net.ssl.HttpsURLConnection

data class VerifiedApk(val file: File, val sha256: String, val version: String)
class AndroidUpdater(context: Context) {
    private val context = context.applicationContext
    private fun connection(url: String): HttpsURLConnection {
        var address = URL(url)
        repeat(6) {
            val host = address.host
            if (address.protocol != "https" || !(host == "api.github.com" || host == "github.com" || host.endsWith(".githubusercontent.com"))) throw SurveyFailure("update_asset")
            val connection = address.openConnection() as HttpsURLConnection
            connection.connectTimeout = 8_000; connection.readTimeout = 10_000; connection.instanceFollowRedirects = false
            connection.setRequestProperty("User-Agent", "WifiMap-Android/${BuildConfig.VERSION_NAME}")
            connection.setRequestProperty("Accept", "application/vnd.github+json")
            val code = connection.responseCode
            if (code in 300..399) { val next = connection.getHeaderField("Location") ?: throw SurveyFailure("update_error"); connection.disconnect(); address = URL(address, next) }
            else { if (code != 200) { connection.disconnect(); throw SurveyFailure("update_error", "HTTP $code") }; return connection }
        }
        throw SurveyFailure("update_error")
    }
    private fun readText(url: String, limit: Int): String {
        val connection = connection(url)
        try { return connection.inputStream.use { it.readBounded(limit, "update_error") }.toString(Charsets.UTF_8) } finally { connection.disconnect() }
    }
    suspend fun check(): AndroidRelease? = withContext(Dispatchers.IO) {
        UpdateRules.release(readText("https://api.github.com/repos/${UpdateRules.REPOSITORY}/releases/latest", 2 * 1024 * 1024), BuildConfig.VERSION_NAME)
    }
    suspend fun download(release: AndroidRelease, progress: (Int) -> Unit): VerifiedApk = withContext(Dispatchers.IO) {
        val expected = release.sha256 ?: readText(release.checksumUrl ?: throw SurveyFailure("update_checksum"), 1024).trim().split(Regex("\\s+")).let { fields ->
            val hash = fields.firstOrNull() ?: throw SurveyFailure("update_checksum")
            if (!hash.matches(Regex("[a-fA-F0-9]{64}")) || (fields.size > 1 && fields[1].trimStart('*') != release.name)) throw SurveyFailure("update_checksum")
            hash.lowercase()
        }
        val directory = context.cacheDir.resolve("updates").also { if (!it.exists() && !it.mkdirs()) throw SurveyFailure("file_error") }
        val temporary = File.createTempFile("download-", ".apk", directory)
        val connection = connection(release.url)
        try {
            var count = 0L; var last = -1; val hash = MessageDigest.getInstance("SHA-256")
            val started = android.os.SystemClock.elapsedRealtime()
            connection.inputStream.use { input -> temporary.outputStream().use { output ->
                val buffer = ByteArray(64 * 1024)
                while (true) {
                    currentCoroutineContext().ensureActive()
                    if (android.os.SystemClock.elapsedRealtime() - started > 120_000) throw SurveyFailure("update_error")
                    val n = input.read(buffer); if (n < 0) break
                    count += n; if (count > release.size) throw SurveyFailure("update_incomplete")
                    hash.update(buffer, 0, n); output.write(buffer, 0, n)
                    val percent = (count * 95 / release.size).toInt(); if (percent != last) { progress(percent); last = percent }
                }
                output.fd.sync()
            } }
            if (count != release.size) throw SurveyFailure("update_incomplete")
            if (hex(hash.digest()) != expected) throw SurveyFailure("update_checksum")
            validatePackage(temporary, release.version.toString()); progress(100)
            VerifiedApk(temporary, expected, release.version.toString())
        } catch (error: Throwable) { temporary.delete(); throw error } finally { connection.disconnect() }
    }
    @Suppress("DEPRECATION")
    private fun packageInfo(path: String?): PackageInfo? {
        val flags = if (Build.VERSION.SDK_INT >= 28) PackageManager.GET_SIGNING_CERTIFICATES else PackageManager.GET_SIGNATURES
        return if (path == null) context.packageManager.getPackageInfo(context.packageName, flags) else context.packageManager.getPackageArchiveInfo(path, flags)
    }
    @Suppress("DEPRECATION")
    private fun signatures(info: PackageInfo): Set<String> = (if (Build.VERSION.SDK_INT >= 28) info.signingInfo?.apkContentsSigners else info.signatures)?.map { hex(MessageDigest.getInstance("SHA-256").digest(it.toByteArray())) }?.toSet().orEmpty()
    @Suppress("DEPRECATION")
    private fun validatePackage(file: File, version: String) {
        val incoming = packageInfo(file.absolutePath) ?: throw SurveyFailure("update_asset")
        val installed = packageInfo(null) ?: throw SurveyFailure("update_asset")
        val newCode = if (Build.VERSION.SDK_INT >= 28) incoming.longVersionCode else incoming.versionCode.toLong()
        val oldCode = if (Build.VERSION.SDK_INT >= 28) installed.longVersionCode else installed.versionCode.toLong()
        if (incoming.packageName != context.packageName || incoming.versionName != version || newCode <= oldCode || (incoming.applicationInfo?.minSdkVersion ?: Int.MAX_VALUE) > Build.VERSION.SDK_INT) throw SurveyFailure("update_asset")
        if (signatures(installed).isEmpty() || signatures(incoming) != signatures(installed)) throw SurveyFailure("android_update_signer")
    }
    fun canInstall(): Boolean = context.packageManager.canRequestPackageInstalls()
    suspend fun install(apk: VerifiedApk) = withContext(Dispatchers.IO) {
        if (!canInstall()) throw SurveyFailure("android_install_permission")
        val digest = MessageDigest.getInstance("SHA-256"); apk.file.inputStream().use { input -> val data = ByteArray(65536); while (true) { val n = input.read(data); if (n < 0) break; digest.update(data, 0, n) } }
        if (hex(digest.digest()) != apk.sha256) throw SurveyFailure("update_checksum")
        validatePackage(apk.file, apk.version)
        val installer = context.packageManager.packageInstaller
        val params = PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL).also {
            it.setAppPackageName(context.packageName)
            if (Build.VERSION.SDK_INT >= 31) it.setRequireUserAction(PackageInstaller.SessionParams.USER_ACTION_REQUIRED)
        }
        val id = installer.createSession(params)
        try {
            installer.openSession(id).use { session ->
                session.openWrite("base.apk", 0, apk.file.length()).use { output -> apk.file.inputStream().use { it.copyTo(output) }; session.fsync(output) }
                val intent = Intent(context, InstallResultReceiver::class.java)
                val flags = PendingIntent.FLAG_UPDATE_CURRENT or if (Build.VERSION.SDK_INT >= 31) PendingIntent.FLAG_MUTABLE else 0
                session.commit(PendingIntent.getBroadcast(context, id, intent, flags).intentSender)
            }
        } catch (error: Throwable) { installer.abandonSession(id); throw error }
    }
    companion object { private fun hex(bytes: ByteArray) = bytes.joinToString("") { "%02x".format(it.toInt() and 255) } }
}
