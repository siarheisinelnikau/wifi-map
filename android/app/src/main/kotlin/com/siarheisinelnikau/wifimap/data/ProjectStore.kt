package com.siarheisinelnikau.wifimap.data

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Uri
import android.util.AtomicFile
import com.siarheisinelnikau.wifimap.domain.*
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import java.io.ByteArrayOutputStream
import java.util.Base64

class ProjectStore(context: Context) {
    private val context = context.applicationContext
    private val resolver = this.context.contentResolver
    private val draft = AtomicFile(this.context.filesDir.resolve("current-project.wifimap"))
    private val draftLock = Mutex()
    private fun read(uri: Uri, limit: Int): ByteArray = resolver.openInputStream(uri)?.use { it.readBounded(limit) } ?: throw SurveyFailure("file_error")
    suspend fun open(uri: Uri): WifiProject = withContext(Dispatchers.IO) {
        ProjectCodec.decode(read(uri, MAX_PROJECT_BYTES).toString(Charsets.UTF_8))
    }
    suspend fun save(uri: Uri, project: WifiProject) = withContext(Dispatchers.IO) {
        val bytes = ProjectCodec.encode(project).toByteArray(Charsets.UTF_8)
        draftLock.withLock { checkpointBytes(bytes) }
        // Document providers control replacement atomicity. The validated private
        // recovery copy is committed before touching the chosen destination.
        resolver.openOutputStream(uri, "wt")?.use { it.write(bytes); it.flush() } ?: throw SurveyFailure("file_error")
    }
    suspend fun checkpoint(project: WifiProject) = withContext(Dispatchers.IO) { val bytes=ProjectCodec.encode(project).toByteArray(Charsets.UTF_8); draftLock.withLock { checkpointBytes(bytes) } }
    suspend fun clearRecovery() = withContext(Dispatchers.IO) { draftLock.withLock { draft.delete() } }
    private fun checkpointBytes(bytes: ByteArray) {
        val output = draft.startWrite()
        try { output.write(bytes); draft.finishWrite(output) } catch (error: Exception) { draft.failWrite(output); throw error }
    }
    suspend fun recovery(): WifiProject? = withContext(Dispatchers.IO) {
        draftLock.withLock { if (!draft.baseFile.exists()) null else draft.openRead().use { ProjectCodec.decode(it.readBounded(MAX_PROJECT_BYTES).toString(Charsets.UTF_8)) } }
    }
    suspend fun importImage(uri: Uri): FloorPlan = withContext(Dispatchers.IO) {
        val bytes = read(uri, MAX_IMAGE_BYTES)
        val bounds = BitmapFactory.Options().also { it.inJustDecodeBounds = true }; BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
        if (bounds.outWidth !in 1..4096 || bounds.outHeight !in 1..4096 || bounds.outMimeType !in listOf("image/png", "image/jpeg")) throw SurveyFailure("image_error")
        val bitmap = BitmapFactory.decodeByteArray(bytes, 0, bytes.size) ?: throw SurveyFailure("image_error")
        try {
            val output = ByteArrayOutputStream()
            if (!bitmap.compress(Bitmap.CompressFormat.PNG, 100, output)) throw SurveyFailure("image_error")
            if (output.size() > MAX_IMAGE_BYTES) throw SurveyFailure("file_too_large")
            FloorPlan(listOf(bitmap.width, bitmap.height), PlanImage("floor-plan.png", Base64.getEncoder().encodeToString(output.toByteArray())))
        } finally { bitmap.recycle() }
    }
    suspend fun displayImage(plan: FloorPlan): Bitmap? = withContext(Dispatchers.IO) {
        val image = plan.image ?: return@withContext null
        val bytes = Base64.getDecoder().decode(image.png)
        val options = BitmapFactory.Options().also { it.inSampleSize = 1 }
        while (plan.size[0] / options.inSampleSize > 2048 || plan.size[1] / options.inSampleSize > 2048) options.inSampleSize *= 2
        BitmapFactory.decodeByteArray(bytes, 0, bytes.size, options) ?: throw SurveyFailure("image_error")
    }
    suspend fun writeText(uri: Uri, text: String) = withContext(Dispatchers.IO) { resolver.openOutputStream(uri, "wt")?.use { it.write(text.toByteArray(Charsets.UTF_8)) } ?: throw SurveyFailure("file_error") }
}
