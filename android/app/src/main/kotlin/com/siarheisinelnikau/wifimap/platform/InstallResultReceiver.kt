package com.siarheisinelnikau.wifimap.platform

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageInstaller
import android.os.Build

class InstallResultReceiver : BroadcastReceiver() {
    @Suppress("DEPRECATION")
    override fun onReceive(context: Context, intent: Intent) {
        when (intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE)) {
            PackageInstaller.STATUS_PENDING_USER_ACTION -> {
                val confirmation = if (Build.VERSION.SDK_INT >= 33) intent.getParcelableExtra(Intent.EXTRA_INTENT, Intent::class.java) else intent.getParcelableExtra(Intent.EXTRA_INTENT)
                if (confirmation != null) context.startActivity(confirmation.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
            }
            PackageInstaller.STATUS_SUCCESS -> context.getSharedPreferences("wifimap-update-result", Context.MODE_PRIVATE).edit().putString("result", "android_update_installed").apply()
            else -> context.getSharedPreferences("wifimap-update-result", Context.MODE_PRIVATE).edit().putString("result", "update_install").putString("details", intent.getIntExtra(PackageInstaller.EXTRA_STATUS, -1).toString()).apply()
        }
    }
}
