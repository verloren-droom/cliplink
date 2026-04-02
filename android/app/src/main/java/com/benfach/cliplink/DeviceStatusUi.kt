package com.benfach.cliplink

import android.content.Context
import androidx.annotation.ColorInt
import androidx.core.content.ContextCompat

data class DeviceStatusUi(
    val label: String,
    @ColorInt val lampColor: Int,
)

fun RustBridge.SettingsDevice.toDeviceStatusUi(context: Context): DeviceStatusUi {
    return when (statusKind) {
        "connected_trusted" -> DeviceStatusUi(
            label = context.getString(R.string.device_status_connected_trusted),
            lampColor = ContextCompat.getColor(context, R.color.color_device_status_connected),
        )
        "trusted_standby" -> DeviceStatusUi(
            label = context.getString(R.string.device_status_trusted_standby),
            lampColor = ContextCompat.getColor(context, R.color.color_device_status_standby),
        )
        "offline" -> DeviceStatusUi(
            label = context.getString(R.string.device_status_offline),
            lampColor = ContextCompat.getColor(context, R.color.color_device_status_offline),
        )
        else -> DeviceStatusUi(
            label = context.getString(R.string.device_status_untrusted),
            lampColor = ContextCompat.getColor(context, R.color.color_device_status_untrusted),
        )
    }
}
