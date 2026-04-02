package com.benfach.cliplink

import android.os.Build
import android.text.format.DateFormat
import android.view.ViewGroup
import android.widget.ImageButton
import android.view.Gravity
import android.view.View
import android.widget.CheckBox
import android.widget.LinearLayout
import android.widget.PopupMenu
import android.widget.ProgressBar
import android.widget.TextView
import androidx.appcompat.app.AlertDialog
import androidx.appcompat.app.AppCompatActivity
import androidx.core.content.ContextCompat
import androidx.core.view.updateLayoutParams
import androidx.recyclerview.widget.LinearLayoutManager
import androidx.recyclerview.widget.RecyclerView
import java.util.Date
import kotlin.math.min

class TransferPanelController(
    private val activity: AppCompatActivity,
    private val onStatusMessage: (String?) -> Unit,
) {
    private val transferFabButton: ImageButton = activity.findViewById(R.id.button_transfer_fab)
    private val transferPopupCard: LinearLayout = activity.findViewById(R.id.transfer_popup_card)
    private val transferCurrentSection: LinearLayout =
        activity.findViewById(R.id.transfer_current_section)
    private val transferPopupFile: TextView = activity.findViewById(R.id.text_transfer_popup_file)
    private val transferPopupProgress: TextView =
        activity.findViewById(R.id.text_transfer_popup_progress)
    private val transferPopupSource: TextView =
        activity.findViewById(R.id.text_transfer_popup_source)
    private val transferPopupProgressBar: ProgressBar =
        activity.findViewById(R.id.progress_transfer_popup)
    private val transferHistoryList: RecyclerView =
        activity.findViewById(R.id.list_transfer_history)
    private val downloadHistoryAdapter = DownloadHistoryAdapter(
        onItemLongPressed = { anchor, record -> showDownloadRecordActions(anchor, record) },
    )

    private var currentTransferProgress: RustBridge.TransferProgress? = null
    private var currentDownloadRecords: List<DownloadHistoryStore.DownloadRecord> = emptyList()
    private var transferPopupExpanded = false
    private var activeTransferItemId: String? = null
    private var lastTransferFinished = false
    private val historyListBaseTopMargin =
        (transferHistoryList.layoutParams as? ViewGroup.MarginLayoutParams)?.topMargin ?: 0
    private val compactScreen = activity.resources.configuration.smallestScreenWidthDp in 0..399

    init {
        transferHistoryList.layoutManager = LinearLayoutManager(activity)
        transferHistoryList.setHasFixedSize(false)
        transferHistoryList.itemAnimator = null
        transferHistoryList.adapter = downloadHistoryAdapter
        downloadHistoryAdapter.registerAdapterDataObserver(
            object : RecyclerView.AdapterDataObserver() {
                override fun onChanged() = requestTransferPopupLayout()

                override fun onItemRangeInserted(positionStart: Int, itemCount: Int) =
                    requestTransferPopupLayout()

                override fun onItemRangeRemoved(positionStart: Int, itemCount: Int) =
                    requestTransferPopupLayout()

                override fun onItemRangeChanged(positionStart: Int, itemCount: Int) =
                    requestTransferPopupLayout()

                override fun onItemRangeMoved(
                    fromPosition: Int,
                    toPosition: Int,
                    itemCount: Int,
                ) = requestTransferPopupLayout()
            },
        )
        transferFabButton.setColorFilter(
            ContextCompat.getColor(activity, R.color.color_text_primary),
        )
        transferFabButton.setOnClickListener {
            if (activeTransferItemId.isNullOrEmpty() && currentDownloadRecords.isEmpty()) {
                return@setOnClickListener
            }
            transferPopupExpanded = !transferPopupExpanded
            transferPopupCard.visibility = if (transferPopupExpanded) View.VISIBLE else View.GONE
            requestTransferPopupLayout()
        }
        applyAdaptiveSizing()
        render(null)
    }

    fun render(progress: RustBridge.TransferProgress?) {
        currentTransferProgress = progress
        currentDownloadRecords = DownloadHistoryStore.listRecords(activity)

        val activeProgress = progress?.takeUnless(::isTransferCompleted)
        lastTransferFinished = when {
            progress != null -> isTransferCompleted(progress)
            currentDownloadRecords.isNotEmpty() -> true
            else -> false
        }

        if (activeProgress == null && currentDownloadRecords.isEmpty()) {
            activeTransferItemId = null
            transferPopupExpanded = false
            transferPopupCard.visibility = View.GONE
            clearTransferCurrentSection()
            renderDownloadHistoryRecords(false)
        }

        if (activeProgress != null) {
            if (activeTransferItemId != activeProgress.itemId) {
                transferPopupExpanded = true
            }
            activeTransferItemId = activeProgress.itemId

            val percent = (activeProgress.fraction.coerceIn(0.0, 1.0) * 100.0).toInt()
            transferCurrentSection.visibility = View.VISIBLE
            transferPopupFile.text = activeProgress.summary.ifBlank {
                activity.getString(R.string.history_file_transfer_progress_placeholder)
            }
            val progressLabel = activity.getString(
                R.string.history_transfer_progress_label,
                "$percent%",
            )
            val sizeLabel = activity.getString(
                R.string.history_transfer_size_label,
                UiFormatters.formatBytes(activeProgress.bytesDone),
                UiFormatters.formatBytes(
                    activeProgress.bytesTotal.coerceAtLeast(activeProgress.bytesDone),
                ),
            )
            transferPopupProgress.text = "$progressLabel\n$sizeLabel"
            transferPopupSource.text = activity.getString(
                R.string.history_transfer_source_label,
                activeProgress.sourceDeviceName.ifBlank { "远端设备" },
            )
            transferPopupProgressBar.progress =
                (activeProgress.fraction.coerceIn(0.0, 1.0) * 1000.0).toInt()
        } else {
            activeTransferItemId = null
            clearTransferCurrentSection()
        }

        renderDownloadHistoryRecords(activeProgress != null)

        val buttonState = when {
            activeProgress != null -> TransferButtonState(
                iconRes = android.R.drawable.stat_sys_download,
                contentDescription = activity.getString(
                    R.string.history_transfer_button_progress_description,
                    (activeProgress.fraction.coerceIn(0.0, 1.0) * 100.0).toInt(),
                ),
            )
            currentDownloadRecords.isNotEmpty() -> TransferButtonState(
                iconRes = android.R.drawable.stat_sys_download_done,
                contentDescription = activity.getString(
                    R.string.history_transfer_button_history_description,
                    currentDownloadRecords.size,
                ),
            )
            lastTransferFinished -> TransferButtonState(
                iconRes = android.R.drawable.stat_sys_download_done,
                contentDescription = activity.getString(R.string.history_transfer_button_done_description),
            )
            else -> TransferButtonState(
                iconRes = android.R.drawable.stat_sys_download_done,
                contentDescription = activity.getString(R.string.history_transfer_button_content_description),
            )
        }
        transferFabButton.setImageResource(buttonState.iconRes)
        transferFabButton.contentDescription = buttonState.contentDescription
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            transferFabButton.tooltipText = buttonState.contentDescription
        }
        transferFabButton.visibility = View.VISIBLE
        transferPopupCard.visibility = if (transferPopupExpanded) View.VISIBLE else View.GONE
        requestTransferPopupLayout()
    }

    fun onPendingClipboardApplied() {
        render(currentTransferProgress)
    }

    private fun clearTransferCurrentSection() {
        transferCurrentSection.visibility = View.GONE
        transferPopupProgressBar.progress = 0
        transferPopupFile.text = ""
        transferPopupProgress.text = ""
        transferPopupSource.text = ""
    }

    private fun renderDownloadHistoryRecords(hasActiveTransfer: Boolean) {
        val records = currentDownloadRecords.toList()
        downloadHistoryAdapter.submitList(records) {
            syncTransferPopupLayout(hasActiveTransfer, records.size)
        }
        val hasRecords = currentDownloadRecords.isNotEmpty()
        transferHistoryList.visibility = if (hasRecords) View.VISIBLE else View.GONE
        syncTransferPopupLayout(hasActiveTransfer, records.size)
    }

    private fun isTransferCompleted(progress: RustBridge.TransferProgress): Boolean {
        return progress.fraction >= 1.0 ||
            (progress.bytesTotal > 0L && progress.bytesDone >= progress.bytesTotal)
    }

    private fun showDownloadRecordActions(
        anchor: View,
        record: DownloadHistoryStore.DownloadRecord,
    ) {
        val popupMenu = PopupMenu(activity, anchor)
        popupMenu.menu.add(
            0,
            MENU_ACTION_REMOVE,
            0,
            activity.getString(R.string.history_transfer_record_remove),
        )
        popupMenu.menu.add(
            0,
            MENU_ACTION_PROPERTIES,
            1,
            activity.getString(R.string.history_transfer_record_menu_properties),
        )
        popupMenu.setOnMenuItemClickListener { menuItem ->
            when (menuItem.itemId) {
                MENU_ACTION_PROPERTIES -> {
                    showDownloadRecordPropertiesDialog(record)
                    true
                }
                MENU_ACTION_REMOVE -> {
                    showRemoveDownloadRecordDialog(record)
                    true
                }
                else -> false
            }
        }
        popupMenu.show()
    }

    private fun showDownloadRecordPropertiesDialog(record: DownloadHistoryStore.DownloadRecord) {
        val createdAt = DateFormat.format("yyyy-MM-dd HH:mm:ss", Date(record.createdAtEpochMs))
            .toString()
        val countText = activity.getString(
            R.string.history_transfer_record_properties_count,
            record.fileCount,
        )
        val message = buildString {
            append(activity.getString(R.string.history_transfer_record_properties_source, record.sourceDeviceName))
            append('\n')
            append(activity.getString(R.string.history_transfer_record_properties_time, createdAt))
            append('\n')
            append(activity.getString(R.string.history_transfer_record_properties_folder, record.folderDisplay))
            append('\n')
            append(
                activity.getString(
                    R.string.history_transfer_record_properties_size,
                    UiFormatters.formatBytes(record.totalBytes),
                ),
            )
            append('\n')
            append(countText)
            if (record.files.isNotEmpty()) {
                append("\n\n")
                append(activity.getString(R.string.history_transfer_record_properties_files))
                record.files.take(MAX_PROPERTY_FILE_LINES).forEach { file ->
                    append("\n• ")
                    append(file.displayName)
                }
                if (record.files.size > MAX_PROPERTY_FILE_LINES) {
                    append('\n')
                    append(
                        activity.getString(
                            R.string.history_transfer_record_properties_files_more,
                            record.files.size,
                        ),
                    )
                }
            }
        }

        AlertDialog.Builder(activity)
            .setTitle(record.title.ifBlank {
                activity.getString(R.string.history_transfer_record_properties_title)
            })
            .setMessage(message)
            .setPositiveButton(android.R.string.ok, null)
            .show()
    }

    private fun showRemoveDownloadRecordDialog(record: DownloadHistoryStore.DownloadRecord) {
        val density = activity.resources.displayMetrics.density
        val horizontalPadding = (24f * density).toInt()
        val topPadding = (8f * density).toInt()
        val checkboxTopMargin = (12f * density).toInt()
        val contentView = LinearLayout(activity).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(horizontalPadding, topPadding, horizontalPadding, 0)
        }
        val messageView = TextView(activity).apply {
            text = activity.getString(R.string.history_transfer_remove_record_message, record.title)
        }
        val deleteFilesCheckbox = CheckBox(activity).apply {
            text = activity.getString(R.string.history_transfer_remove_record_delete_files)
            isChecked = false
            layoutParams = LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.WRAP_CONTENT,
                LinearLayout.LayoutParams.WRAP_CONTENT,
            ).apply {
                gravity = Gravity.CENTER_HORIZONTAL
                topMargin = checkboxTopMargin
            }
        }
        contentView.addView(messageView)
        contentView.addView(deleteFilesCheckbox)

        AlertDialog.Builder(activity)
            .setTitle(activity.getString(R.string.history_transfer_remove_record_title))
            .setView(contentView)
            .setPositiveButton(activity.getString(R.string.history_transfer_remove_record_confirm)) { _, _ ->
                val result = DownloadHistoryStore.removeRecord(
                    activity,
                    record.recordId,
                    deleteFilesCheckbox.isChecked,
                )
                if (!result.removed) {
                    onStatusMessage(activity.getString(R.string.history_transfer_remove_record_failed))
                    return@setPositiveButton
                }
                val message = when {
                    !deleteFilesCheckbox.isChecked ->
                        activity.getString(R.string.history_transfer_remove_record_removed_only)
                    result.failedDeletes == 0 ->
                        activity.getString(
                            R.string.history_transfer_remove_record_removed_with_files,
                            result.deletedFiles,
                        )
                    else ->
                        activity.getString(
                            R.string.history_transfer_remove_record_removed_partial,
                            result.deletedFiles,
                            result.failedDeletes,
                        )
                }
                onStatusMessage(message)
                render(currentTransferProgress)
            }
            .setNegativeButton(android.R.string.cancel, null)
            .show()
    }

    private fun applyAdaptiveSizing() {
        val metrics = activity.resources.displayMetrics
        val popupHorizontalMarginPx = (metrics.density * if (compactScreen) 28f else 36f).toInt()
        val popupMaxWidthPx = (metrics.density * if (compactScreen) 340f else 376f).toInt()
        val popupMinWidthPx = (metrics.density * 248f).toInt()
        val targetPopupWidth = min(
            (metrics.widthPixels - popupHorizontalMarginPx).coerceAtLeast(popupMinWidthPx),
            popupMaxWidthPx,
        )
        transferPopupCard.updateLayoutParams<ViewGroup.LayoutParams> {
            width = targetPopupWidth
        }

        val buttonSizePx = (metrics.density * if (compactScreen) 48f else 52f).toInt()
        transferFabButton.updateLayoutParams<ViewGroup.LayoutParams> {
            width = buttonSizePx
            height = buttonSizePx
        }
    }

    private fun syncTransferPopupLayout(hasActiveTransfer: Boolean, recordCount: Int) {
        val desiredHeight = resolveHistoryListHeightPx(recordCount)
        transferHistoryList.updateLayoutParams<ViewGroup.MarginLayoutParams> {
            topMargin = if (hasActiveTransfer) historyListBaseTopMargin else 0
            height = desiredHeight
        }
        transferHistoryList.isNestedScrollingEnabled =
            desiredHeight == maxHistoryListHeightPx() && recordCount > 0
        requestTransferPopupLayout()
    }

    private fun resolveHistoryListHeightPx(recordCount: Int): Int {
        if (recordCount <= 0) {
            return ViewGroup.LayoutParams.WRAP_CONTENT
        }
        val rowHeightPx = (activity.resources.displayMetrics.density * 72f).toInt()
        return min(maxHistoryListHeightPx(), rowHeightPx * recordCount)
    }

    private fun maxHistoryListHeightPx(): Int {
        val metrics = activity.resources.displayMetrics
        val screenFraction = if (compactScreen) 0.24f else 0.30f
        val fromScreen = (metrics.heightPixels * screenFraction).toInt()
        val cap = (metrics.density * if (compactScreen) 192f else 240f).toInt()
        return min(fromScreen, cap).coerceAtLeast((metrics.density * 96f).toInt())
    }

    private fun requestTransferPopupLayout() {
        applyAdaptiveSizing()
        transferPopupCard.post {
            transferHistoryList.requestLayout()
            transferPopupCard.requestLayout()
            (transferPopupCard.parent as? View)?.requestLayout()
        }
    }

    private data class TransferButtonState(
        val iconRes: Int,
        val contentDescription: String,
    )

    private companion object {
        private const val MENU_ACTION_PROPERTIES = 1
        private const val MENU_ACTION_REMOVE = 2
        private const val MAX_PROPERTY_FILE_LINES = 8
    }
}
