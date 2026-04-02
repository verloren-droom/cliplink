package com.benfach.cliplink

object UiFormatters {
    fun formatBytes(bytes: Long): String {
        if (bytes < 1024L) {
            return "$bytes B"
        }

        val units = arrayOf("KB", "MB", "GB", "TB")
        var value = bytes.toDouble()
        var unitIndex = -1
        while (value >= 1024.0 && unitIndex < units.lastIndex) {
            value /= 1024.0
            unitIndex += 1
        }
        return String.format("%.2f %s", value, units[unitIndex.coerceAtLeast(0)])
    }
}
