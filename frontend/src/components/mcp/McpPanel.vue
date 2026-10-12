<script setup lang="ts">
import type { McpServer } from '@/generated/admin-api'
import type { McpServerWorkspaceView } from '@/models'
import McpServerListPanel from './McpServerListPanel.vue'

defineProps<{
  busy: boolean
  mcpFirst: number
  mcpRows: number
  /**
   * Which rows are gated, and by which action. A pass-through only: the page
   * owns the map and this panel just carries it to the rows.
   */
  pendingKeys?: Readonly<Record<string, string>>
  t: TranslateFn
  testingMcpServerId: string
  workspaceView: McpServerWorkspaceView
}>()

defineEmits<{
  deleteMcpServer: [server: McpServer]
  editMcpServer: [server: McpServer]
  mcpPage: [event: TablePageChange]
  testMcpServer: [server: McpServer]
  toggleMcpServer: [server: McpServer]
}>()
</script>

<template>
  <McpServerListPanel
    :busy="busy"
    :list-items="workspaceView.list_items"
    :mcp-first="mcpFirst"
    :mcp-rows="mcpRows"
    :mcp-total="workspaceView.total_count"
    :pending-keys="pendingKeys"
    :t="t"
    :testing-mcp-server-id="testingMcpServerId"
    @delete-mcp-server="$emit('deleteMcpServer', $event)"
    @edit-mcp-server="$emit('editMcpServer', $event)"
    @mcp-page="$emit('mcpPage', $event)"
    @test-mcp-server="$emit('testMcpServer', $event)"
    @toggle-mcp-server="$emit('toggleMcpServer', $event)"
  />
</template>
