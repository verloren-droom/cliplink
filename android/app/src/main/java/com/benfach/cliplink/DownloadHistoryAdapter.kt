package com.benfach.cliplink

import android.content.Context
import android.text.format.DateFormat
import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.widget.TextView
import androidx.recyclerview.widget.DiffUtil
import androidx.recyclerview.widget.ListAdapter
import androidx.recyclerview.widget.RecyclerView
import java.util.Date

class DownloadHistoryAdapter(
    private val onItemLongPressed: (View, DownloadHistoryStore.DownloadRecord) -> Unit,
) : ListAdapter<DownloadHistoryStore.DownloadRecord, DownloadHistoryAdapter.DownloadRecordViewHolder>(
    DownloadRecordDiffCallback,
) {

    override fun onCreateViewHolder(parent: ViewGroup, viewType: Int): DownloadRecordViewHolder {
        val view = LayoutInflater.from(parent.context)
            .inflate(R.layout.item_download_record, parent, false)
        return DownloadRecordViewHolder(view, onItemLongPressed)
    }

    override fun onBindViewHolder(holder: DownloadRecordViewHolder, position: Int) {
        holder.bind(getItem(position))
    }

    class DownloadRecordViewHolder(
        itemView: View,
        private val onItemLongPressed: (View, DownloadHistoryStore.DownloadRecord) -> Unit,
    ) : RecyclerView.ViewHolder(itemView) {
        private val context: Context = itemView.context
        private val titleView: TextView = itemView.findViewById(R.id.text_download_record_title)
        private val metaView: TextView = itemView.findViewById(R.id.text_download_record_meta)
        private var currentRecord: DownloadHistoryStore.DownloadRecord? = null

        init {
            itemView.setOnLongClickListener { view ->
                currentRecord?.let { record ->
                    onItemLongPressed(view, record)
                    true
                } ?: false
            }
        }

        fun bind(record: DownloadHistoryStore.DownloadRecord) {
            currentRecord = record
            titleView.text = record.title
            metaView.text = buildMetaText(record)
        }

        private fun buildMetaText(record: DownloadHistoryStore.DownloadRecord): String {
            val createdAt = DateFormat.format("yyyy-MM-dd HH:mm", Date(record.createdAtEpochMs))
                .toString()
            val sizeText = context.getString(
                R.string.history_transfer_record_size,
                UiFormatters.formatBytes(record.totalBytes),
            )
            return context.getString(
                R.string.history_transfer_record_meta_compact,
                record.sourceDeviceName,
                sizeText,
                createdAt,
            )
        }
    }

    private object DownloadRecordDiffCallback :
        DiffUtil.ItemCallback<DownloadHistoryStore.DownloadRecord>() {
        override fun areItemsTheSame(
            oldItem: DownloadHistoryStore.DownloadRecord,
            newItem: DownloadHistoryStore.DownloadRecord,
        ): Boolean = oldItem.recordId == newItem.recordId

        override fun areContentsTheSame(
            oldItem: DownloadHistoryStore.DownloadRecord,
            newItem: DownloadHistoryStore.DownloadRecord,
        ): Boolean = oldItem == newItem
    }
}
