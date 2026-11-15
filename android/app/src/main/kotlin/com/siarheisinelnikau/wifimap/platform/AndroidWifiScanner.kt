package com.siarheisinelnikau.wifimap.platform

import android.Manifest
import android.annotation.SuppressLint
import android.content.*
import android.content.pm.PackageManager
import android.location.LocationManager
import android.net.wifi.WifiManager
import android.os.Build
import android.os.SystemClock
import androidx.core.content.ContextCompat
import com.siarheisinelnikau.wifimap.domain.*
import kotlinx.coroutines.*
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

class AndroidWifiScanner(context: Context) : ScanPort {
    private val context = context.applicationContext
    private val manager = this.context.getSystemService(WifiManager::class.java)
    private val throttle = ScanThrottle()
    fun permissions(): Array<String> = if (Build.VERSION.SDK_INT >= 33) arrayOf(Manifest.permission.ACCESS_COARSE_LOCATION, Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.NEARBY_WIFI_DEVICES) else arrayOf(Manifest.permission.ACCESS_COARSE_LOCATION, Manifest.permission.ACCESS_FINE_LOCATION)
    fun permissionGranted(): Boolean = permissions().filterNot { it == Manifest.permission.ACCESS_COARSE_LOCATION }.all { ContextCompat.checkSelfPermission(context, it) == PackageManager.PERMISSION_GRANTED }
    @Suppress("DEPRECATION")
    @SuppressLint("MissingPermission")
    private fun connected(): List<Int>? = parseBssid(manager?.connectionInfo?.bssid)
    @Suppress("DEPRECATION")
    @SuppressLint("MissingPermission")
    override suspend fun scan(target: List<Int>?): List<Observation> = withContext(Dispatchers.Main.immediate) {
        if (manager == null || !manager.isWifiEnabled) throw SurveyFailure("android_wifi_off")
        if (!permissionGranted()) throw SurveyFailure("android_permission")
        val location = context.getSystemService(LocationManager::class.java)
        val enabled = if (Build.VERSION.SDK_INT >= 28) location.isLocationEnabled else location.isProviderEnabled(LocationManager.NETWORK_PROVIDER) || location.isProviderEnabled(LocationManager.GPS_PROVIDER)
        if (!enabled) throw SurveyFailure("android_location_off")
        val wait = throttle.reserve(SystemClock.elapsedRealtime())
        if (wait > 0) throw SurveyFailure("android_scan_throttled", wait.toString())
        val before = connected()
        val requested = SystemClock.elapsedRealtimeNanos() / 1000
        val rows = try {
            withTimeout(12_000) {
                var receiver: BroadcastReceiver? = null
                try {
                    suspendCancellableCoroutine<List<Observation>> { continuation ->
                        receiver = object : BroadcastReceiver() {
                            override fun onReceive(unused: Context, intent: Intent) {
                                if (!continuation.isActive || intent.action != WifiManager.SCAN_RESULTS_AVAILABLE_ACTION) return
                                if (!intent.getBooleanExtra(WifiManager.EXTRA_RESULTS_UPDATED, false)) { continuation.resumeWithException(SurveyFailure("android_scan_throttled")); return }
                                try {
                                    val clock = ScanClock(requested, SystemClock.elapsedRealtimeNanos() / 1000, System.currentTimeMillis())
                                    val results = manager.scanResults.mapNotNull { row ->
                                        val bssid = parseBssid(row.BSSID) ?: return@mapNotNull null
                                        if (target != null && bssid != target) return@mapNotNull null
                                        val raw = Build.VERSION.SDK_INT >= 33 && row.wifiSsid != null
                                        val bytes = if (Build.VERSION.SDK_INT >= 33) row.wifiSsid?.bytes ?: byteArrayOf() else row.SSID.orEmpty().toByteArray(Charsets.UTF_8)
                                        if (bytes.size > 32) return@mapNotNull null
                                        try { ScanRules.observation(clock, row.timestamp, bytes.map { it.toInt() and 255 }, bssid, row.level, row.frequency, raw, before) } catch (_: SurveyFailure) { null }
                                    }.distinctBy { it.bssid }
                                    if (results.isEmpty()) continuation.resumeWithException(SurveyFailure("wlan_stale")) else continuation.resume(results)
                                } catch (failure: SecurityException) { continuation.resumeWithException(SurveyFailure("android_permission")) }
                            }
                        }
                        ContextCompat.registerReceiver(context, receiver, IntentFilter(WifiManager.SCAN_RESULTS_AVAILABLE_ACTION), ContextCompat.RECEIVER_EXPORTED)
                        try { if (!manager.startScan()) continuation.resumeWithException(SurveyFailure("android_scan_throttled")) }
                        catch (_: SecurityException) { if (continuation.isActive) continuation.resumeWithException(SurveyFailure("android_permission")) }
                    }
                } finally { receiver?.let { runCatching { context.unregisterReceiver(it) } } }
            }
        } catch (_: TimeoutCancellationException) { throw SurveyFailure("android_scan_timeout") }
        ScanRules.checkConnection(before, connected())
        rows
    }
}
