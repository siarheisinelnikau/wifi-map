package com.siarheisinelnikau.wifimap.presentation

import android.graphics.Color

data class Palette(val background: Int, val surface: Int, val text: Int, val muted: Int, val border: Int, val accent: Int, val selected: Int) {
    companion object {
        fun of(dark: Boolean) = if (dark) Palette(Color.rgb(16,28,37), Color.rgb(24,40,50), Color.rgb(230,240,240), Color.rgb(161,183,192), Color.rgb(59,80,91), Color.rgb(54,170,151), Color.rgb(31,78,76)) else Palette(Color.rgb(239,244,245), Color.rgb(250,252,252), Color.rgb(25,48,62), Color.rgb(79,102,113), Color.rgb(207,220,223), Color.rgb(11,115,111), Color.rgb(213,239,232))
    }
}
