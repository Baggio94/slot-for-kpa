package fyi.slot.konkr

import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.util.Log
import androidx.core.content.FileProvider
import org.json.JSONArray
import java.io.File
import java.io.FileOutputStream
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean

/**
 * Foreground-only: never schedules JobScheduler/alarms, wakes the device, or
 * interrupts libretro. Release notes, update check and installer are explicit
 * controller actions; discovery is due only once in 24 hours.
 */
internal class SlotUpdater(private val context: Context, private val notify: (Int, String) -> Unit) {
    companion object {
        const val CURRENT = 0
        const val AVAILABLE = 1
        const val CHECKING = 2
        const val DOWNLOADING = 3
        const val FAILED = 4
        const val READY = 5
        private const val TAG = "SlotUpdater"

        private const val API = "https://api.github.com/repos/Baggio94/slot-for-kpa/releases?per_page=20"
        private const val RELEASES = "https://github.com/Baggio94/slot-for-kpa/releases/"
        private const val MAX_APK = 150L * 1024 * 1024
        private val worker = Executors.newSingleThreadExecutor { task ->
            Thread(task, "slot-updater").apply { priority = Thread.MIN_PRIORITY }
        }

        private fun allowedAsset(raw: String): Boolean {
            val u = runCatching { URL(raw) }.getOrNull() ?: return false
            return u.protocol == "https" &&
                u.host.equals("github.com", true) &&
                u.path.startsWith("/Baggio94/slot-for-kpa/releases/download/") &&
                !u.path.contains("..") && u.query == null && u.ref == null
        }

        private fun bytes(url: String, limit: Int): ByteArray {
            val allowed = url == API || allowedAsset(url)
            require(allowed) { "Unexpected release URL" }
            val connection = (URL(url).openConnection() as HttpURLConnection).apply {
                connectTimeout = 8_000
                readTimeout = 12_000
                instanceFollowRedirects = true
                setRequestProperty("User-Agent", "slot-konkr-update")
            }
            return try {
                val status = connection.responseCode
                if (status != 200) {
                    // Retry only transient service/rate-limit failures;
                    // permanent errors must not hammer the GitHub API.
                    if (status == 429 || status in 500..599) {
                        throw IOException("GitHub HTTP $status")
                    }
                    error("GitHub HTTP $status")
                }
                connection.inputStream.use { input ->
                    val out = java.io.ByteArrayOutputStream()
                    val buffer = ByteArray(8192)
                    while (true) {
                        val read = input.read(buffer)
                        if (read < 0) break
                        require(out.size() + read <= limit) { "Release metadata too large" }
                        out.write(buffer, 0, read)
                    }
                    out.toByteArray()
                }
            } finally { connection.disconnect() }
        }
    }

    private data class Release(val version: String, val page: String, val apk: String, val sha: String)
    private val prefs = context.getSharedPreferences("slot-updater-v1", Context.MODE_PRIVATE)
    private val running = AtomicBoolean(false)
    private var latest: Release? = null

    private fun version() = context.packageManager.getPackageInfo(
        context.packageName, 0).versionName.orEmpty()

    fun enabled(): Boolean = prefs.getBoolean("enabled", true)
    fun toggle(): Boolean {
        val value = !enabled()
        prefs.edit().putBoolean("enabled", value).apply()
        return value
    }

    private fun publish(status: Int, text: String = "") {
        Handler(Looper.getMainLooper()).post { notify(status, text.take(48)) }
    }

    private fun network(): Boolean = runCatching {
        val manager = context.getSystemService(ConnectivityManager::class.java)
        val caps = manager.getNetworkCapabilities(manager.activeNetwork)
        caps != null && caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED)
    }.getOrDefault(false)

    fun onForeground() {
        // Cached metadata must be tied to the installed version that fetched it.
        if (prefs.getString("installed", "") == version()) {
            val name = prefs.getString("latest", "").orEmpty()
            val page = prefs.getString("page", "").orEmpty()
            val apk = prefs.getString("apk", "").orEmpty()
            val sha = prefs.getString("sha", "").orEmpty()
            if (SlotUpdatePolicy.newer(name, version()) &&
                page.startsWith(RELEASES) && allowedAsset(apk) &&
                sha.matches(Regex("[0-9a-fA-F]{64}"))) {
                latest = Release(name, page, apk, sha)
            }
        }
        publish(if (latest == null) CURRENT else AVAILABLE, latest?.version ?: version())
        check(false)
    }

    fun refreshVisibleStatus() {
        publish(if (latest == null) CURRENT else AVAILABLE, latest?.version ?: version())
    }

    /**
     * A manual button press is never failed just because Android has not yet
     * marked a freshly-connected Wi-Fi as VALIDATED. Do the brief readiness
     * wait/retries on the existing low-priority worker, never on the UI thread.
     */
    private fun findReleaseWithRetry(): Release? {
        for (attempt in 1..3) {
            if (!network()) {
                if (attempt == 3) throw IOException("Android Internet network not yet validated")
                Log.i(TAG, "Network not yet validated, waiting before GitHub check (attempt $attempt)")
                Thread.sleep(attempt * 650L)
                continue
            }
            try {
                return findRelease()
            } catch (error: IOException) {
                if (attempt == 3) throw error
                Log.w(TAG, "Transient GitHub update check failure; retry $attempt/2", error)
                Thread.sleep(attempt * 650L)
            }
        }
        throw IOException("Unable to check GitHub release metadata")
    }

    fun check(manual: Boolean) {
        if (!manual && !enabled()) return
        // Automatic checks wait for a validated network; explicit checks may
        // start during Android's brief network-validation window.
        if (!manual && !network()) return
        val now = System.currentTimeMillis()
        if (!manual && !SlotUpdatePolicy.due(now, prefs.getLong("success", 0L),
                prefs.getLong("failure", 0L))) return
        if (!running.compareAndSet(false, true)) return
        publish(CHECKING)
        worker.execute {
            try {
                val found = findReleaseWithRetry()
                prefs.edit().putLong("success", System.currentTimeMillis())
                    .remove("failure").apply()
                latest = found
                if (found != null) {
                    prefs.edit().putString("installed", version())
                        .putString("latest", found.version).putString("page", found.page)
                        .putString("apk", found.apk).putString("sha", found.sha).apply()
                    publish(AVAILABLE, found.version)
                } else {
                    prefs.edit().remove("latest").remove("page").remove("apk")
                        .remove("sha").apply()
                    publish(CURRENT, version())
                }
            } catch (error: Exception) {
                Log.w(TAG, "Update check failed after readiness/retry checks", error)
                prefs.edit().putLong("failure", System.currentTimeMillis()).apply()
                val cached = latest
                publish(if (cached == null) FAILED else AVAILABLE, cached?.version.orEmpty())
            } finally { running.set(false) }
        }
    }

    private fun findRelease(): Release? {
        val all = JSONArray(String(bytes(API, 768 * 1024)))
        var best: Release? = null
        for (i in 0 until all.length()) {
            val release = all.optJSONObject(i) ?: continue
            if (release.optBoolean("draft")) continue
            val tag = release.optString("tag_name")
            if (!tag.matches(Regex("""konkr-v\d+\.\d+\.\d+(-[A-Za-z0-9.-]+)?"""))) continue
            val next = tag.removePrefix("konkr-v")
            if (!SlotUpdatePolicy.eligible(version(), next,
                    release.optBoolean("prerelease"))) continue
            if (best != null && !SlotUpdatePolicy.newer(next, best.version)) continue
            val page = release.optString("html_url")
            if (!page.startsWith(RELEASES)) continue
            val assets = release.optJSONArray("assets") ?: continue
            var apk = ""
            var checksumUrl = ""
            var digest = ""
            for (j in 0 until assets.length()) {
                val asset = assets.optJSONObject(j) ?: continue
                val name = asset.optString("name")
                val uri = asset.optString("browser_download_url")
                if (!allowedAsset(uri)) continue
                if (name == "SHA256SUMS.txt") checksumUrl = uri
                if (name.startsWith("slot-for-konkr-") &&
                    name.endsWith("-arm64.apk") && name.contains(next)) {
                    apk = uri
                    digest = asset.optString("digest").removePrefix("sha256:").lowercase()
                }
            }
            if (apk.isBlank()) continue
            var sha = digest.takeIf { it.matches(Regex("[a-f0-9]{64}")) }
            if (sha == null && checksumUrl.isNotBlank()) {
                val expectedName = apk.substringAfterLast('/')
                val sumText = String(bytes(checksumUrl, 32 * 1024))
                sha = sumText.lineSequence().mapNotNull {
                    val parts = Regex("""^([0-9a-fA-F]{64})\s+\*?(\S+)$""")
                        .matchEntire(it.trim()) ?: return@mapNotNull null
                    parts.groupValues[1].lowercase()
                        .takeIf { _ -> parts.groupValues[2] == expectedName }
                }.firstOrNull()
            }
            if (sha == null) continue
            best = Release(next, page, apk, sha)
        }
        return best
    }

    fun openNotes() {
        val url = latest?.page ?: RELEASES
        runCatching {
            context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)))
        }
    }

    /** Called ONLY after pressing A on Install in Slot's native UI. */
    fun downloadAndInstall(onVerified: (File) -> Unit) {
        val release = latest ?: return
        if (!running.compareAndSet(false, true)) return
        publish(DOWNLOADING, release.version)
        worker.execute {
            val destination = File(context.cacheDir, "slot-updates").apply { mkdirs() }
                .resolve("slot-update.apk")
            try {
                require(allowedAsset(release.apk))
                val conn = (URL(release.apk).openConnection() as HttpURLConnection).apply {
                    connectTimeout = 10_000
                    readTimeout = 20_000
                    instanceFollowRedirects = true
                    setRequestProperty("User-Agent", "slot-konkr-update")
                }
                try {
                    require(conn.responseCode == 200)
                    require(conn.contentLengthLong < 0 || conn.contentLengthLong <= MAX_APK)
                    val digest = MessageDigest.getInstance("SHA-256")
                    var count = 0L
                    conn.inputStream.use { input ->
                        FileOutputStream(destination).use { output ->
                            val buf = ByteArray(32 * 1024)
                            while (true) {
                                val n = input.read(buf)
                                if (n < 0) break
                                count += n
                                require(count <= MAX_APK)
                                digest.update(buf, 0, n)
                                output.write(buf, 0, n)
                            }
                            output.fd.sync()
                        }
                    }
                    val computed = digest.digest().joinToString("") {
                        "%02x".format(it.toInt() and 255)
                    }
                    require(count > 1024 && computed == release.sha) { "SHA-256 mismatch" }
                    require(validPackage(destination)) { "APK package/signature/version mismatch" }
                } finally { conn.disconnect() }
                publish(READY, release.version)
                Handler(Looper.getMainLooper()).post { onVerified(destination) }
            } catch (_: Exception) {
                destination.delete()
                publish(FAILED, release.version)
            } finally { running.set(false) }
        }
    }

    private fun validPackage(file: File): Boolean {
        val pm = context.packageManager
        val flags = PackageManager.GET_SIGNING_CERTIFICATES
        val downloaded = pm.getPackageArchiveInfo(file.absolutePath, flags) ?: return false
        val installed = pm.getPackageInfo(context.packageName, flags)
        if (downloaded.packageName != context.packageName ||
            downloaded.longVersionCode <= installed.longVersionCode) return false
        val newSigners = downloaded.signingInfo?.apkContentsSigners ?: return false
        val oldSigners = installed.signingInfo?.apkContentsSigners ?: return false
        return oldSigners.isNotEmpty() && oldSigners.map { it.toByteArray().toList() }.toSet() ==
            newSigners.map { it.toByteArray().toList() }.toSet()
    }

    fun startInstaller(file: File) {
        val uri = FileProvider.getUriForFile(context, "fyi.slot.konkr.updates", file)
        context.startActivity(Intent(Intent.ACTION_VIEW).apply {
            setDataAndType(uri, "application/vnd.android.package-archive")
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        })
    }
}
