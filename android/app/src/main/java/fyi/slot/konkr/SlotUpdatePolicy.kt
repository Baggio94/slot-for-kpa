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
        val n = maxOf(a.pre.size, b.pre.size)
        for (i in 0 until n) {
            val left = a.pre.getOrNull(i) ?: return true.not()
            val right = b.pre.getOrNull(i) ?: return true
            val ln = left.toIntOrNull()
            val rn = right.toIntOrNull()
            val comparison = when {
                ln != null && rn != null -> ln.compareTo(rn)
                ln != null -> -1
                rn != null -> 1
                else -> left.compareTo(right, ignoreCase = true)
            }
            if (comparison != 0) return comparison > 0
        }
        return false
    }

    /** Public stable builds never fetch prerelease updates. */
    fun eligible(current: String, candidate: String, prerelease: Boolean): Boolean =
        (!prerelease || parse(current)?.pre != null) && newer(candidate, current)
}
