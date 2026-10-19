package fyi.slot.konkr

import android.content.Context
import android.net.Uri
import org.json.JSONObject

/**
 * Read-only SleepManager status protocol v1.
 *
 * Never controls SleepManager. An absent or unsupported provider is UNKNOWN,
 * and must not be interpreted as the user having disabled SleepManager.
 */
internal object SleepManagerStatusClient {
    const val API_VERSION = 1
    val URI: Uri = Uri.parse("content://com.med.sleepmanager.status")

    data class Status(val enabled: Boolean, val serviceRunning: Boolean)

    fun parse(raw: String?): Status? {
        if (raw.isNullOrBlank()) return null
        return runCatching {
            val value = JSONObject(raw)
            if (value.optInt("apiVersion", -1) != API_VERSION) return null
            val enabled = value.opt("enabled")
            val running = value.opt("serviceRunning")
            if (enabled !is Boolean || running !is Boolean) return null
            Status(enabled, running)
        }.getOrNull()
    }

    /**
     * ContentResolver.call() can be cross-process IPC. Call on a worker, NEVER
     * from the GLES/UI render loop.
     */
    fun read(context: Context): Status? = runCatching {
        val reply = context.contentResolver.call(URI, "status", null, null)
        parse(reply?.getString("status"))
    }.getOrNull()
}
