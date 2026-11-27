<template>
  <div class="forensic-db-viewer">
    <div v-if="loading" class="state-row mono"><Loader2 :size="14" class="spin" /> {{ tr("正在解析二进制日志 / 键值库…") }}</div>
    <div v-else-if="error" class="state-row mono error">{{ error }}</div>

    <!-- systemd-journal 原生视图 -->
    <template v-else-if="mode === 'journal' && journalDigest">
      <div class="fdb-toolbar mono">
        <span class="ui-badge xs outline">systemd-journal ({{ journalDigest.state }})</span>
        <span>{{ tr("已解析 {0} / 声明 {1} 条", { 0: journalDigest.parsedEntries, 1: journalDigest.declaredEntries }) }}</span>
        <span v-if="journalDigest.compactMode" class="ui-badge xs outline">COMPACT</span>
        <span class="dim">machine={{ journalDigest.machineId.slice(0, 12) }}…</span>
      </div>
      <div class="fdb-scroll">
        <table class="data-table db-table">
          <thead>
            <tr>
              <th>#</th>
              <th>{{ tr("时间 (UTC+8)") }}</th>
              <th>{{ tr("级别") }}</th>
              <th>{{ tr("单元 / 进程") }}</th>
              <th>PID</th>
              <th>{{ tr("消息内容") }}</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="rec in journalDigest.records" :key="rec.seqnum">
              <td class="mono dim">{{ rec.seqnum }}</td>
              <td class="mono">{{ rec.timestampUtc8 }}</td>
              <td><span class="ui-badge xs outline">P{{ rec.priority }}</span></td>
              <td class="mono">{{ rec.unit || rec.identifier || '—' }}</td>
              <td class="mono dim">{{ rec.pid || '—' }}</td>
              <td class="mono">{{ rec.message }}</td>
            </tr>
          </tbody>
        </table>
      </div>
    </template>

    <!-- LevelDB / IndexedDB 原生视图 -->
    <template v-else-if="mode === 'leveldb' && leveldbDigest">
      <div class="fdb-toolbar mono">
        <span class="ui-badge xs outline">LevelDB ({{ leveldbDigest.filesParsed.join(', ') }})</span>
        <span>{{ tr("存活 {0} · 墓碑 {1}", { 0: leveldbDigest.liveEntries, 1: leveldbDigest.deletedEntries }) }}</span>
        <span v-if="leveldbDigest.metamaskVaults > 0" class="ui-badge xs tone" style="--badge-tone: var(--sev-warning-fg)">
          MetaMask Vault ({{ leveldbDigest.metamaskVaults }})
        </span>
        <span v-if="leveldbDigest.fernetTokens > 0" class="ui-badge xs tone" style="--badge-tone: var(--chart-2)">
          Fernet ({{ leveldbDigest.fernetTokens }})
        </span>
        <label class="fdb-chk">
          <input v-model="showDeleted" type="checkbox" @change="load" /> {{ tr("含已删除 Tombstone") }}
        </label>
      </div>
      <div class="fdb-scroll">
        <table class="data-table db-table">
          <thead>
            <tr>
              <th>Seq</th>
              <th>{{ tr("状态") }}</th>
              <th>Key</th>
              <th>Value / {{ tr("结构解码") }}</th>
              <th>{{ tr("特征标签") }}</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="(entry, idx) in leveldbDigest.entries" :key="idx">
              <td class="mono dim">{{ entry.sequence }}</td>
              <td>
                <span v-if="entry.deleted" class="ui-badge xs danger">{{ tr("已删除") }}</span>
                <span v-else class="ui-badge xs outline">{{ tr("存活") }}</span>
              </td>
              <td class="mono">{{ entry.key }}</td>
              <td class="mono">
                <div v-if="entry.decodedBlob">{{ entry.decodedBlob.summary }}</div>
                <div v-else>{{ entry.valuePreview }}</div>
              </td>
              <td class="mono">
                <span v-if="entry.metamaskVault" class="ui-badge xs warn">MetaMask</span>
                <span v-if="entry.fernet" class="ui-badge xs outline" :title="entry.fernet.tokenPrefix">
                  Fernet @ {{ entry.fernet.timestampUtc8 }}
                </span>
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    </template>

    <!-- PostgreSQL PGDATA / 8 KiB Heap 原生视图 -->
    <template v-else-if="mode === 'pgdata' && pgRows">
      <div class="fdb-toolbar mono">
        <span class="ui-badge xs outline">PostgreSQL ({{ pgRows.tableName }})</span>
        <span>{{ tr("存活 {0} · 死元组(已删/旧版) {1}", { 0: pgRows.totalLive, 1: pgRows.totalDead }) }}</span>
        <label class="fdb-chk">
          <input v-model="showDeleted" type="checkbox" @change="load" /> {{ tr("含死元组 (xmax≠0)") }}
        </label>
      </div>
      <div class="fdb-scroll">
        <table class="data-table db-table">
          <thead>
            <tr>
              <th>Page:Item</th>
              <th>xmin / xmax</th>
              <th>{{ tr("状态") }}</th>
              <th v-for="col in pgRows.columns" :key="col.attnum" class="mono">
                {{ col.name }} <span class="dim">({{ col.typeName }})</span>
              </th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="(row, ri) in pgRows.rows" :key="ri">
              <td class="mono dim">{{ row.page }}:{{ row.itemIndex }}</td>
              <td class="mono dim">{{ row.xmin }} / {{ row.xmax }}</td>
              <td>
                <span v-if="row.deleted" class="ui-badge xs danger">{{ tr("死元组") }}</span>
                <span v-else class="ui-badge xs outline">{{ tr("存活") }}</span>
              </td>
              <td v-for="(val, vi) in row.values" :key="vi" class="mono">{{ val }}</td>
            </tr>
          </tbody>
        </table>
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { ref, watch } from 'vue';
import { Loader2 } from 'lucide-vue-next';
import { tr } from '../../../../composables/useLocale';
import type { ImageId, NodeId } from '../../../../lib/ipc/ids';
import {
  fsJournalPreview, fsLeveldbPreview, fsPgdataRows,
  type JournalDigest, type LevelDbDigest, type PgTableRows,
} from '../../../../lib/ipc/forensicPreview';

const props = defineProps<{
  imageId: ImageId;
  nodeId: NodeId;
  parentId?: NodeId | null;
  mode: 'journal' | 'leveldb' | 'pgdata';
  query?: string;
}>();

const loading = ref(false);
const error = ref<string | null>(null);
const showDeleted = ref(true);
const journalDigest = ref<JournalDigest | null>(null);
const leveldbDigest = ref<LevelDbDigest | null>(null);
const pgRows = ref<PgTableRows | null>(null);

async function load() {
  loading.value = true;
  error.value = null;
  try {
    if (props.mode === 'journal') {
      journalDigest.value = await fsJournalPreview({
        imageId: props.imageId,
        nodeId: props.nodeId,
        query: props.query || null,
        limit: 300,
      });
    } else if (props.mode === 'leveldb') {
      leveldbDigest.value = await fsLeveldbPreview({
        imageId: props.imageId,
        nodeId: props.nodeId,
        parentId: props.parentId ?? null,
        query: props.query || null,
        includeDeleted: showDeleted.value,
        limit: 300,
      });
    } else if (props.mode === 'pgdata') {
      pgRows.value = await fsPgdataRows({
        imageId: props.imageId,
        nodeId: props.nodeId,
        parentId: props.parentId ?? null,
        includeDead: showDeleted.value,
        offset: 0,
        limit: 200,
      });
    }
  } catch (err) {
    error.value = String(err);
  } finally {
    loading.value = false;
  }
}

watch(() => [props.imageId, props.nodeId, props.mode, props.query], () => {
  void load();
}, { immediate: true });
</script>

<style scoped src="../../../../styles/data-table.css"></style>
<style scoped>
.forensic-db-viewer {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  overflow: hidden;
}
.fdb-toolbar {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 6px 12px;
  font-size: 11px;
  border-bottom: 1px solid var(--border-color);
  background: var(--surface-secondary);
  flex-wrap: wrap;
}
.fdb-chk {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  margin-left: auto;
  cursor: pointer;
}
.fdb-scroll {
  flex: 1;
  min-height: 0;
  overflow: auto;
}
</style>
