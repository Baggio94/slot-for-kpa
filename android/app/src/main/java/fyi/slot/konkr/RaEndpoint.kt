package fyi.slot.konkr

import android.content.Context
import android.net.Uri
import org.json.JSONObject

/** Read-only RAOfflineProxy endpoint discovery; never starts/stops the service. */
internal object RaEndpoint {
  private val uri = Uri.parse("content://com.raofflineproxy.config")
  enum class Mode { OFF, DIRECT, PROXY }
  data class Status(
    val running: Boolean, val host: String, val port: Int,
    val online: Boolean?, val pendingAwards: Int?
  ) {
    fun base(): String? = if (running && host == "127.0.0.1" && port in 1024..65535)
      "http://127.0.0.1:" + port else null
  }
  sealed interface Route {
    data object Off : Route
    data class Available(val endpoint: String, val throughProxy: Boolean) : Route
    data object ProxyUnavailable : Route
  }
  fun select(mode: Mode, status: Status?): Route = when (mode) {
    Mode.OFF -> Route.Off
    Mode.DIRECT -> Route.Available("https://retroachievements.org/dorequest.php", false)
    Mode.PROXY -> status?.base()?.let { Route.Available(it + "/dorequest.php", true) }
      ?: Route.ProxyUnavailable
  }

  /**
   * Prepared for the native rcheevos worker: only its authenticated API
   * request may mark apiReachable=true. Never infer it from Wi-Fi strength.
   * In automatic mode, a prepared library is read through our native ledger;
   * RAOfflineProxy is used only after the user explicitly selects it.
   *
   * This plans transport and does not enable achievements by itself.
   */
  fun plan(
    preference: RaRouting.Preference,
    apiReachable: Boolean,
    accountRemembered: Boolean,
    gamePrepared: Boolean,
    proxy: Status?
  ): RaRouting.Plan = RaRouting.plan(preference, RaRouting.Observed(
    apiReachable = apiReachable,
    accountRemembered = accountRemembered,
    gamePrepared = gamePrepared,
    proxyEndpoint = proxy?.base()?.plus("/dorequest.php")
  ))
  /** Call only from a worker thread. The status API requires no control permission. */
  fun discover(context: Context): Status? {
    return try {
    val r = context.contentResolver
    val listener = r.query(uri, null, null, null, null)?.use { c ->
      if (!c.moveToFirst()) return@use null
      val host = c.getColumnIndex("proxy_host")
      val port = c.getColumnIndex("proxy_port")
      val running = c.getColumnIndex("proxy_running")
      if (host < 0 || port < 0 || running < 0) return@use null
      Status(c.getInt(running) != 0, c.getString(host), c.getInt(port), null, null)
    } ?: return null
    val json = r.call(uri, "status", null, null)?.getString("status")
      ?.let { JSONObject(it) }
    Status(listener.running, listener.host, listener.port,
      if (json != null && json.has("online")) json.optBoolean("online") else null,
      json?.optJSONObject("pendingAwards")?.optInt("count"))
    } catch (_: Exception) { null }
  }
}
