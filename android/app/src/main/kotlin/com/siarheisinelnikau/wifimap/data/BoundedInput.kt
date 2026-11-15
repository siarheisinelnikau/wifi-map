package com.siarheisinelnikau.wifimap.data

import com.siarheisinelnikau.wifimap.domain.SurveyFailure
import java.io.ByteArrayOutputStream
import java.io.InputStream

internal fun InputStream.readBounded(limit: Int, failureKey: String = "file_too_large"): ByteArray {
    require(limit >= 0)
    val output = ByteArrayOutputStream()
    val buffer = ByteArray(16 * 1024)
    while (true) {
        val count = read(buffer)
        if (count < 0) break
        if (count > limit - output.size()) throw SurveyFailure(failureKey)
        output.write(buffer, 0, count)
    }
    return output.toByteArray()
}
