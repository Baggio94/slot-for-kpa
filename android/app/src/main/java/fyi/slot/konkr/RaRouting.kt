package fyi.slot.konkr

/**
 * The transport policy for the future native RetroAchievements engine.
 *
 * Planning is intentionally pure: no credentials, network requests, or
 * achievement writes occur here. In particular, a Wi-Fi toggle or Android's
 * "validated network" flag is NOT proof the RetroAchievements API is reachable.
 * The RA network worker must supply apiReachable from its own HTTPS result.
 *
 * Native mode owns one persistent award ledger. When offline it MUST keep
 * awarding locally from previously verified definitions and preserve pending
 * awards until the real server acknowledges them. Never queue an award in
 * RAOfflineProxy *and* the native ledger.
 */
internal object RaRouting {
    enum class Preference {
        OFF,
        /** Direct HTTPS online, privately cached rcheevos evaluation offline. */
        AUTOMATIC,
        /** Opt-in external RAOfflineProxy backend; one award owner at a time. */
        RAOFFLINEPROXY
    }

    data class Observed(
        val apiReachable: Boolean,
        val accountRemembered: Boolean,
        val gamePrepared: Boolean,
        /** Derived from RAOfflineProxy's verified loopback content provider. */
        val proxyEndpoint: String? = null
    )

    sealed interface Plan {
        data object Disabled : Plan
        data object Direct : Plan
        data object NativeOffline : Plan
        data object NeedsPreparation : Plan
        data class Proxy(val endpoint: String) : Plan
        data object ProxyUnavailable : Plan
    }

    private fun checkedProxyEndpoint(endpoint: String?): String? {
        if (endpoint == null) return null
        // Do not allow arbitrary remote HTTP hosts or a changed
        // RA server URL to receive a private emulator login token.
        val match = Regex("^http://127\\.0\\.0\\.1:([0-9]{4,5})/dorequest\\.php$")
            .matchEntire(endpoint) ?: return null
        val port = match.groupValues[1].toIntOrNull() ?: return null
        return endpoint.takeIf { port in 1024..65535 }
    }

    fun plan(preference: Preference, state: Observed): Plan = when (preference) {
        Preference.OFF -> Plan.Disabled
        Preference.RAOFFLINEPROXY ->
            checkedProxyEndpoint(state.proxyEndpoint)?.let(Plan::Proxy)
                ?: Plan.ProxyUnavailable
        Preference.AUTOMATIC -> when {
            state.apiReachable -> Plan.Direct
            state.accountRemembered && state.gamePrepared -> Plan.NativeOffline
            else -> Plan.NeedsPreparation
        }
    }
}
