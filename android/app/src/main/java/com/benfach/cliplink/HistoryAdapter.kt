package com.benfach.cliplink

import android.content.Context
import androidx.core.content.ContextCompat
import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.widget.TextView
import androidx.recyclerview.widget.DiffUtil
import androidx.recyclerview.widget.ListAdapter
import androidx.recyclerview.widget.RecyclerView

class HistoryAdapter(
    private val onItemClicked: (RustBridge.HistoryItem) -> Unit,
    private val onItemLongPressed: (View, RustBridge.HistoryItem) -> Unit,
) : ListAdapter<RustBridge.HistoryItem, HistoryAdapter.HistoryViewHolder>(HistoryDiffCallback) {

    override fun onCreateViewHolder(parent: ViewGroup, viewType: Int): HistoryViewHolder {
        val view = LayoutInflater.from(parent.context)
            .inflate(R.layout.item_history, parent, false)
        return HistoryViewHolder(view, onItemClicked, onItemLongPressed)
    }

    override fun onBindViewHolder(holder: HistoryViewHolder, position: Int) {
        holder.bind(getItem(position))
    }

    init {
        setHasStableIds(true)
    }

    override fun getItemId(position: Int): Long = getItem(position).id.hashCode().toLong()

    class HistoryViewHolder(
        itemView: View,
        private val onItemClicked: (RustBridge.HistoryItem) -> Unit,
        private val onItemLongPressed: (View, RustBridge.HistoryItem) -> Unit,
    ) : RecyclerView.ViewHolder(itemView) {
        private val context: Context = itemView.context
        private val summaryView: TextView = itemView.findViewById(R.id.text_title)
        private val detailView: TextView = itemView.findViewById(R.id.text_detail)
        private val sourceView: TextView = itemView.findViewById(R.id.text_source)
        private var currentItem: RustBridge.HistoryItem? = null

        init {
            itemView.setOnClickListener {
                currentItem?.let(onItemClicked)
            }
            itemView.setOnLongClickListener { view ->
                currentItem?.let { item ->
                    onItemLongPressed(view, item)
                    true
                } ?: false
            }
        }

        fun bind(item: RustBridge.HistoryItem) {
            currentItem = item
            summaryView.text = item.summaryText.ifBlank {
                context.getString(R.string.empty_history_summary)
            }
            val detailParts = buildList {
                add(
                    when (item.kind) {
                        "files" -> context.getString(R.string.history_item_kind_files)
                        else -> context.getString(R.string.history_item_kind_text)
                    },
                )
                if (item.isPinned && !item.isRemote) {
                    add(context.getString(R.string.history_item_pinned_label))
                }
            }
            detailView.text = detailParts.joinToString(" · ")
            detailView.visibility = if (detailParts.isEmpty()) View.GONE else View.VISIBLE
            sourceView.text = item.sourceBadge
            sourceView.contentDescription = item.sourceTooltip ?: item.sourceBadge
            sourceView.setBackgroundResource(
                if (item.isRemote) {
                    R.drawable.bg_history_badge_remote
                } else {
                    R.drawable.bg_history_badge_local
                },
            )
            sourceView.setTextColor(
                ContextCompat.getColor(
                    context,
                    if (item.isRemote) {
                        R.color.color_badge_remote_text
                    } else {
                        R.color.color_badge_local_text
                    },
                ),
            )
        }
    }

    private object HistoryDiffCallback : DiffUtil.ItemCallback<RustBridge.HistoryItem>() {
        override fun areItemsTheSame(
            oldItem: RustBridge.HistoryItem,
            newItem: RustBridge.HistoryItem,
        ): Boolean = oldItem.id == newItem.id

        override fun areContentsTheSame(
            oldItem: RustBridge.HistoryItem,
            newItem: RustBridge.HistoryItem,
        ): Boolean = oldItem == newItem
    }
}
