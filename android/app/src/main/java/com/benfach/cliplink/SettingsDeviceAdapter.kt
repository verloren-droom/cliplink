package com.benfach.cliplink

import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.widget.CheckBox
import android.widget.TextView
import androidx.recyclerview.widget.RecyclerView

class SettingsDeviceAdapter(
    private val onSelectionChanged: (RustBridge.SettingsDevice, Boolean) -> Unit,
    private val onItemLongPressed: (View, RustBridge.SettingsDevice) -> Unit,
) : RecyclerView.Adapter<SettingsDeviceAdapter.SettingsDeviceViewHolder>() {
    private var devices: List<RustBridge.SettingsDevice> = emptyList()
    private var selectedDeviceIds: Set<String> = emptySet()

    override fun onCreateViewHolder(parent: ViewGroup, viewType: Int): SettingsDeviceViewHolder {
        val view = LayoutInflater.from(parent.context)
            .inflate(R.layout.item_settings_device, parent, false)
        return SettingsDeviceViewHolder(view, onSelectionChanged, onItemLongPressed)
    }

    override fun onBindViewHolder(holder: SettingsDeviceViewHolder, position: Int) {
        holder.bind(getItem(position), isSelected = getItem(position).deviceId in selectedDeviceIds)
    }

    override fun getItemCount(): Int = devices.size

    fun submitDevices(devices: List<RustBridge.SettingsDevice>, selectedDeviceIds: Set<String>) {
        this.devices = devices
        this.selectedDeviceIds = selectedDeviceIds.toSet()
        notifyDataSetChanged()
    }

    private fun getItem(position: Int): RustBridge.SettingsDevice = devices[position]

    class SettingsDeviceViewHolder(
        itemView: View,
        private val onSelectionChanged: (RustBridge.SettingsDevice, Boolean) -> Unit,
        private val onItemLongPressed: (View, RustBridge.SettingsDevice) -> Unit,
    ) : RecyclerView.ViewHolder(itemView) {
        private val nameView: TextView = itemView.findViewById(R.id.text_device_name)
        private val detailView: TextView = itemView.findViewById(R.id.text_device_detail)
        private val statusLampView: TextView = itemView.findViewById(R.id.text_device_status_lamp)
        private val checkBox: CheckBox = itemView.findViewById(R.id.checkbox_device_select)
        private var currentDevice: RustBridge.SettingsDevice? = null

        init {
            itemView.setOnClickListener {
                currentDevice?.let { device ->
                    val nextChecked = !checkBox.isChecked
                    checkBox.isChecked = nextChecked
                    onSelectionChanged(device, nextChecked)
                }
            }
            checkBox.setOnClickListener {
                currentDevice?.let { device ->
                    onSelectionChanged(device, checkBox.isChecked)
                }
            }
            itemView.setOnLongClickListener { view ->
                currentDevice?.let { device ->
                    onItemLongPressed(view, device)
                    true
                } ?: false
            }
        }

        fun bind(device: RustBridge.SettingsDevice, isSelected: Boolean) {
            val statusUi = device.toDeviceStatusUi(itemView.context)
            currentDevice = device
            nameView.text = device.deviceName
            detailView.text = buildString {
                append(statusUi.label)
                if (device.secondaryText.isNotBlank()) {
                    append(" · ")
                    append(device.secondaryText)
                }
            }
            statusLampView.setTextColor(statusUi.lampColor)
            checkBox.isChecked = isSelected
        }
    }
}
