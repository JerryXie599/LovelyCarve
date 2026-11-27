<template>
<section v-if="backup" class="ui-card harmony-backup-card" data-section="harmony-backup" aria-labelledby="harmony-backup-title">
  <header class="ui-card-header">
    <h3 id="harmony-backup-title" class="ui-card-title"><Smartphone :size="15" />{{ tr("HarmonyOS NEXT 备份") }}</h3>
    <span class="ui-badge sm" :class="backup.locked ? 'warn' : 'success'">
      <LockKeyhole v-if="backup.locked" :size="12" />
      <LockOpen v-else :size="12" />
      {{ backup.locked ? tr("已加密（待解锁）") : (backup.declared_encrypted ? tr("已解锁") : tr("未加密")) }}
    </span>
    <span v-if="backup.encryption_type > 0" class="ui-badge sm mono">{{ `Type ${backup.encryption_type}` }}</span>
    <span class="ui-count">{{ backup.module_count }}</span>
  </header>

  <div class="card-body">
    <dl class="hm-kv-grid">
      <div v-if="backup.device_model" class="hm-kv">
        <dt>{{ tr("设备型号") }}</dt>
        <dd class="mono">{{ backup.device_model }}<template v-if="backup.product_model">{{ ` (${backup.product_model})` }}</template></dd>
      </div>
      <div v-if="backup.version_name" class="hm-kv">
        <dt>{{ tr("备份版本") }}</dt>
        <dd class="mono">{{ backup.version_name }}</dd>
      </div>
      <div v-if="backup.backup_time_utc8" class="hm-kv">
        <dt>{{ tr("备份时间（UTC+8）") }}</dt>
        <dd class="mono">{{ backup.backup_time_utc8 }}</dd>
      </div>
      <div class="hm-kv">
        <dt>{{ tr("备份模块") }}</dt>
        <dd class="mono">{{ tr("{0} 个模块", { 0: backup.module_count }) }}<template v-if="backup.clone_modules.length > 0">{{ tr("（含 {0} 个应用分身）", { 0: backup.clone_modules.length }) }}</template></dd>
      </div>
      <div v-if="backup.file_count !== null" class="hm-kv">
        <dt>{{ tr("已索引文件") }}</dt>
        <dd class="mono">{{ backup.file_count.toLocaleString() }}</dd>
      </div>
      <div v-if="backup.backup_reminder" class="hm-kv">
        <dt>{{ tr("密码提示") }}</dt>
        <dd>{{ backup.backup_reminder }}</dd>
      </div>
    </dl>

    <div v-if="backup.clone_modules.length > 0" class="clone-row">
      <span class="clone-label">{{ tr("检测到分身应用：") }}</span>
      <span v-for="clone in backup.clone_modules" :key="clone" class="ui-badge sm mono">{{ clone }}</span>
    </div>

    <form v-if="backup.locked" class="unlock-form" @submit.prevent="unlock">
      <label class="sr-only" for="harmony-backup-pw">{{ tr("输入 HarmonyOS 备份密码") }}</label>
      <input
        id="harmony-backup-pw"
        v-model="password"
        type="password"
        class="ui-input"
        autocomplete="off"
        :placeholder="tr('输入备份密码解锁 HarmonyOS NEXT 加密模块与 TAR…')"
        :disabled="busy"
      />
      <button type="submit" class="ui-btn primary" :disabled="busy || !password">
        <Loader2 v-if="busy" :size="14" class="spin" />
        <LockOpen v-else :size="14" />
        {{ tr("解锁备份") }}
      </button>
      <button v-if="busy" type="button" class="ui-btn ghost" :disabled="cancelling" @click="cancel">
        {{ tr("取消") }}
      </button>
    </form>

    <p v-if="error" class="ui-alert error" role="alert">{{ error }}</p>
  </div>
</section>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { Loader2, LockKeyhole, LockOpen, Smartphone } from 'lucide-vue-next';
import { tr } from '../../../composables/useLocale';
import type { ImageInfo } from '../../../lib/ipc/image';
import { useHarmonyBackup } from '../composables/useHarmonyBackup';

const props = defineProps<{ info: ImageInfo }>();
const emit = defineEmits<{ 'open-image': [path: string] }>();

const backup = computed(() => props.info.harmony_backup ?? null);
const { password, error, busy, cancelling, unlock, cancel } = useHarmonyBackup(
  () => props.info,
  path => emit('open-image', path),
);
</script>

<style scoped>
.card-body { display: flex; flex-direction: column; gap: 12px; }
.hm-kv-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
  gap: 8px 16px;
  margin: 0;
}
.hm-kv { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
.hm-kv dt { font-size: 11px; color: var(--text-secondary); }
.hm-kv dd { margin: 0; font-size: 13px; color: var(--text-primary); word-break: break-word; }
.clone-row { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; font-size: 12px; color: var(--text-secondary); }
.clone-label { font-weight: 500; }
.unlock-form { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
.unlock-form .ui-input { flex: 1 1 240px; min-width: 180px; }
</style>
