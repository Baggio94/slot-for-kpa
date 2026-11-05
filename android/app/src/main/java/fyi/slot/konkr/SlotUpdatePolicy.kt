package fyi.slot.konkr

/** No Android dependencies: update decisions are independently regression tested. */
internal object SlotUpdatePolicy {
    const val DAY_MS = 24L * 60 * 60 * 1000
    const val RETRY_MS = 30L * 60 * 1000

    fun due(now: Long, lastSuccess: Long, lastFailure: Long): Boolean =
        (lastSuccess <= 0 || now < lastSuccess || now - lastSuccess >= DAY_MS) &&
            (lastFailure <= 0 || now < lastFailure || now - lastFailure >= RETRY_MS)

    internal data class Version(val major: Int, val minor: Int, val patch: Int,
                                val pre: List<String>?)

    fun parse(raw: String): Version? {
        val normal = raw.removePrefix("konkr-v").removePrefix("v").trim()
        val match = Regex("""^(\d+)\.(\d+)\.(\d+)(?:-([A-Za-z0-9.-]+))?$""")
            .matchEntire(normal) ?: return null
        val nums = (1..3).map { match.groupValues[it].toIntOrNull() ?: return null }
        return Version(nums[0], nums[1], nums[2],
            match.groupValues[4].takeIf { it.isNotEmpty() }?.split('.', '-'))
    }

    fun newer(candidate: String, installed: String): Boolean {
        val a = parse(candidate) ?: return false
        val b = parse(installed) ?: return false
        for (comparison in listOf(a.major.compareTo(b.major),
            a.minor.compareTo(b.minor), a.patch.compareTo(b.patch))) {
            if (comparison != 0) return comparison > 0
        }
        if (a.pre == null) return b.pre != null
        if (b.pre == null) return false
        // beta10 must sort AFTER beta2, not before it lexicographically.
        val pieces = Regex("""(?<=\D)(?=\d)|(?<=\d)(?=\D)""")
        val left = a.pre.flatMap { it.split(pieces) }
        val right = b.pre.flatMap { it.split(pieces) }
        for (i in 0 until maxOf(left.size, right.size)) {
            val l = left.getOrNull(i) ?: return false
            val r = right.getOrNull(i) ?: return true
            val ln = l.toIntOrNull()
            val rn = r.toIntOrNull()
            val result = when {
                ln != null && rn != null -> ln.compareTo(rn)
                ln != null -> -1
                rn != null -> 1
                else -> l.compareTo(r, ignoreCase = true)
            }
            if (result != 0) return result > 0
        }
        return false
    }

    /**
     * The repository was renamed from slot-for-kpa to slot-for-android, its
     * tags from konkr-vX.Y.Z to vX.Y.Z and its APKs from slot-for-konkr-* to
     * slot-for-android-*. Both names are accepted so no install is stranded.
     */
    private val REPOS = listOf("slot-for-android", "slot-for-kpa")
    private val TAG = Regex("""^(?:konkr-)?v(\d+\.\d+\.\d+(?:-[A-Za-z0-9.-]+)?)$""")

    fun releasePage(url: String): Boolean =
        REPOS.any { url.startsWith("https://github.com/Baggio94/$it/releases/") }

    fun assetPath(path: String): Boolean =
        REPOS.any { path.startsWith("/Baggio94/$it/releases/download/") }

    /** The version a release tag names, or null for any other tag. */
    fun tagVersion(tag: String): String? = TAG.matchEntire(tag)?.groupValues?.get(1)

    /** 2 for the current APK name, 1 for the old one, 0 for anything else. */
    fun apkRank(name: String, version: String): Int = when (name) {
        "slot-for-android-$version-arm64.apk" -> 2
        "slot-for-konkr-$version-arm64.apk" -> 1
        else -> 0
    }

    /** Public stable builds never fetch prerelease updates. */
    fun eligible(current: String, candidate: String, prerelease: Boolean): Boolean =
        ((!prerelease && parse(candidate)?.pre == null) || parse(current)?.pre != null) &&
            newer(candidate, current)
}
