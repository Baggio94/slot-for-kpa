package fyi.slot.konkr

import org.junit.Assert.assertEquals
import org.junit.Test

class RaRoutingTest {
    private fun state(
        online: Boolean = false,
        account: Boolean = false,
        prepared: Boolean = false,
        proxy: String? = null
    ) = RaRouting.Observed(online, account, prepared, proxy)

    @Test fun disabledStaysDisabledEvenWithNetworkAndProxy() {
        assertEquals(RaRouting.Plan.Disabled,
            RaRouting.plan(RaRouting.Preference.OFF, state(
                online = true, account = true, prepared = true,
                proxy = "http://127.0.0.1:8080/dorequest.php")))
    }

    @Test fun automaticUsesRealServerWhenConfirmedReachable() {
        assertEquals(RaRouting.Plan.Direct,
            RaRouting.plan(RaRouting.Preference.AUTOMATIC, state(
                online = true, account = true, prepared = true,
                proxy = "http://127.0.0.1:8080/dorequest.php")))
    }

    @Test fun automaticKeepsTrackingWithoutInternetOnlyIfPrepared() {
        assertEquals(RaRouting.Plan.NativeOffline,
            RaRouting.plan(RaRouting.Preference.AUTOMATIC, state(
                account = true, prepared = true)))
    }

    @Test fun wifiAloneIsNotEnoughToClaimServerAccess() {
        assertEquals(RaRouting.Plan.NativeOffline,
            RaRouting.plan(RaRouting.Preference.AUTOMATIC, state(
                online = false, account = true, prepared = true)))
    }

    @Test fun coldOfflineNeverClaimsUncachedAchievements() {
        assertEquals(RaRouting.Plan.NeedsPreparation,
            RaRouting.plan(RaRouting.Preference.AUTOMATIC, state(
                online = false, account = true, prepared = false)))
    }

    @Test fun anonymousCacheCannotProduceAccountAwards() {
        assertEquals(RaRouting.Plan.NeedsPreparation,
            RaRouting.plan(RaRouting.Preference.AUTOMATIC, state(
                online = false, account = false, prepared = true)))
    }

    @Test fun automaticDoesNotSilentlySwitchLedgerOwners() {
        assertEquals(RaRouting.Plan.NativeOffline,
            RaRouting.plan(RaRouting.Preference.AUTOMATIC, state(
                account = true, prepared = true,
                proxy = "http://127.0.0.1:8080/dorequest.php")))
    }

    @Test fun proxyIsOnlyUsedWhenExplicitlySelected() {
        assertEquals(RaRouting.Plan.Proxy("http://127.0.0.1:8080/dorequest.php"),
            RaRouting.plan(RaRouting.Preference.RAOFFLINEPROXY, state(
                proxy = "http://127.0.0.1:8080/dorequest.php")))
    }

    @Test fun proxyNeverFallsBackToDirectOrNativeWhenUnavailable() {
        assertEquals(RaRouting.Plan.ProxyUnavailable,
            RaRouting.plan(RaRouting.Preference.RAOFFLINEPROXY, state(
                online = true, account = true, prepared = true)))
    }

    @Test fun proxyRejectsRemoteOrSpoofedHostsAndUrls() {
        for (endpoint in listOf(
            "http://192.168.1.2:8080/dorequest.php",
            "http://localhost:8080/dorequest.php",
            "http://127.0.0.1.evil.com:8080/dorequest.php",
            "https://retroachievements.org/dorequest.php",
            "http://127.0.0.1:8080/elsewhere",
            "http://127.0.0.1:8080/dorequest.php?token=foo",
            "http://127.0.0.1:80/dorequest.php",
            "http://127.0.0.1:99999/dorequest.php"
        )) {
            assertEquals("Rejected endpoint: $endpoint", RaRouting.Plan.ProxyUnavailable,
                RaRouting.plan(RaRouting.Preference.RAOFFLINEPROXY, state(proxy = endpoint)))
        }
    }

    @Test fun proxyOnlyUsesValidPortsAndLoopback() {
        for (port in listOf(1024, 8080, 65535)) {
            val endpoint = "http://127.0.0.1:$port/dorequest.php"
            assertEquals(RaRouting.Plan.Proxy(endpoint),
                RaRouting.plan(RaRouting.Preference.RAOFFLINEPROXY, state(proxy = endpoint)))
        }
    }
}
