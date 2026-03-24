package com.benfach.cliplink

import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.widget.TextView
import androidx.recyclerview.widget.RecyclerView

class HistoryAdapter(
    private val onItemClicked: (RustBridge.HistoryItem) -> Unit,
) : RecyclerView.Adapter<HistoryAdapter.HistoryViewHolder>() {
    private val items = mutableListOf<RustBridge.HistoryItem>()

    fun submitList(values: List<RustBridge.HistoryItem>) {
        items.clear()
        items.addAll(values)
        notifyDataSetChanged()
    }

    override fun onCreateViewHolder(parent: ViewGroup, viewType: Int): HistoryViewHolder {
        val view = LayoutInflater.from(parent.context)
            .inflate(R.layout.item_history, parent, false)
        return HistoryViewHolder(view, onItemClicked)
    }

    override fun onBindViewHolder(holder: HistoryViewHolder, position: Int) {
        holder.bind(items[position])
    }

    override fun getItemCount(): Int = items.size

    class HistoryViewHolder(
        itemView: View,
        private val onItemClicked: (RustBridge.HistoryItem) -> Unit,
    ) : RecyclerView.ViewHolder(itemView) {
        private val summaryView: TextView = itemView.findViewById(R.id.text_title)
        private val detailView: TextView = itemView.findViewById(R.id.text_detail)
        private val sourceView: TextView = itemView.findViewById(R.id.text_source)
        private var currentItem: RustBridge.HistoryItem? = null

        init {
            itemView.setOnClickListener {
                currentItem?.let(onItemClicked)
            }
        }

        fun bind(item: RustBridge.HistoryItem) {
            currentItem = item
            summaryView.text = item.summaryText
            detailView.text = item.detailTooltip?.lineSequence()?.firstOrNull().orEmpty()
            detailView.visibility = if (detailView.text.isNullOrEmpty()) View.GONE else View.VISIBLE
            sourceView.text = item.sourceBadge
            sourceView.contentDescription = item.sourceTooltip ?: item.sourceBadge
        }
    }
}
