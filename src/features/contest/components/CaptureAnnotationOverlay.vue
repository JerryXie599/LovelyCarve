<template>
  <div v-if="captureSession" class="capture-overlay">
    <!-- 绘制层：直接覆盖在原版 LovelyCarve 窗口画面之上，截图时保持标注框和箭头留在原生画面内 -->
    <svg
      class="capture-canvas"
      @mousedown="onPointerDown"
      @mousemove="onPointerMove"
      @mouseup="onPointerUp"
    >
      <defs>
        <marker
          id="lc-capture-arrow"
          viewBox="0 0 10 10"
          refX="8"
          refY="5"
          markerWidth="7"
          markerHeight="7"
          orient="auto-start-reverse"
        >
          <path d="M 0 1 L 10 5 L 0 9 z" :fill="color" />
        </marker>
      </defs>

      <!-- 裁剪区域预览暗化遮罩（仅在选定裁剪范围且未隐藏工具栏时提示，实际截图按选定区域截取） -->
      <rect
        v-if="hasBox && (kind === 'box' || kind === 'box_arrow')"
        :x="box.x"
        :y="box.y"
        :width="box.w"
        :height="box.h"
        fill="rgba(0, 0, 0, 0)"
        :stroke="color"
        stroke-width="2.5"
        rx="4"
      />

      <line
        v-if="hasArrow && (kind === 'arrow' || kind === 'box_arrow')"
        :x1="arrowFrom[0]"
        :y1="arrowFrom[1]"
        :x2="arrowTo[0]"
        :y2="arrowTo[1]"
        :stroke="color"
        stroke-width="3"
        marker-end="url(#lc-capture-arrow)"
      />

      <g v-if="labelText.trim() && (hasBox || hasArrow)">
        <rect
          :x="labelPos.x"
          :y="labelPos.y - 22"
          :width="Math.max(80, labelText.trim().length * 13 + 18)"
          height="22"
          rx="4"
          :fill="color"
        />
        <text :x="labelPos.x + 8" :y="labelPos.y - 7" fill="#ffffff" font-size="12" font-weight="600">
          {{ labelText.trim() }}
        </text>
      </g>
    </svg>

    <!-- 悬浮控制条：真实捕获瞬间隐藏，避免把自己截入证据图片 -->
    <div v-if="!capturing" class="capture-toolbar" @mousedown.stop>
      <span class="toolbar-title" :title="captureSession.pin.title">
        {{ captureSession.pin.title }}
      </span>

      <select v-model="kind" class="ui-select sm" :title="tr('标注方式')">
        <option value="box_arrow">{{ tr("红框 + 箭头") }}</option>
        <option value="box">{{ tr("仅红框") }}</option>
        <option value="arrow">{{ tr("仅箭头") }}</option>
        <option value="none">{{ tr("无标注") }}</option>
      </select>

      <select v-model="toolMode" class="ui-select sm" :title="tr('当前鼠标拖拽行为')">
        <option value="box">{{ tr("拖拽框选区域") }}</option>
        <option value="arrow">{{ tr("拖拽绘制箭头") }}</option>
      </select>

      <select v-model="color" class="ui-select sm" :title="tr('标注颜色')">
        <option value="#ef4444">{{ tr("红色") }}</option>
        <option value="#f59e0b">{{ tr("琥珀") }}</option>
        <option value="#2563eb">{{ tr("蓝色") }}</option>
      </select>

      <input
        v-model="labelText"
        type="text"
        class="ui-input sm label-input"
        :placeholder="tr('可选标注文字')"
      />

      <select v-model="scope" class="ui-select sm" :title="tr('截图范围')">
        <option value="full">{{ tr("截取整窗") }}</option>
        <option value="crop" :disabled="!hasBox">{{ tr("仅裁出框选区") }}</option>
      </select>

      <button class="ui-btn ghost sm" type="button" @click="clearDrawing">{{ tr("重置") }}</button>
      <button class="ui-btn ghost sm" type="button" @click="cancel">{{ tr("取消") }}</button>
      <button class="ui-btn primary sm" type="button" @click="confirmCapture">
        <Camera :size="14" />
        <span>{{ tr("截取真实画面") }}</span>
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, reactive, ref, watch } from 'vue';
import { Camera } from 'lucide-vue-next';
import { tr } from '../../../composables/useLocale';
import { notifyError, notifySuccess } from '../../../composables/useToast';
import type { CaptureRegion } from '../../../lib/ipc/desktopCapture';
import { useContestWorkspace } from '../composables/useContestWorkspace';

const { captureSession, finishPinCapture, cancelPinCapture } = useContestWorkspace();

const kind = ref<'box' | 'arrow' | 'box_arrow' | 'none'>('box_arrow');
const toolMode = ref<'box' | 'arrow'>('box');
const color = ref('#ef4444');
const labelText = ref('');
const scope = ref<'full' | 'crop'>('full');
const capturing = ref(false);

const box = reactive({ x: 0, y: 0, w: 0, h: 0 });
const arrowFrom = ref<[number, number]>([0, 0]);
const arrowTo = ref<[number, number]>([0, 0]);
const hasArrow = ref(false);
const dragState = ref<{ mode: 'box' | 'arrow'; startX: number; startY: number } | null>(null);

const hasBox = computed(() => box.w >= 8 && box.h >= 8);

const labelPos = computed(() => {
  if (hasBox.value) {
    return { x: box.x, y: Math.max(26, box.y - 4) };
  }
  return { x: arrowFrom.value[0], y: Math.max(26, arrowFrom.value[1] - 4) };
});

watch(captureSession, session => {
  if (!session) return;
  const saved = session.pin.annotation;
  if (saved) {
    kind.value = (saved.kind as 'box' | 'arrow' | 'box_arrow' | 'none') || 'box_arrow';
    box.x = saved.x;
    box.y = saved.y;
    box.w = saved.w;
    box.h = saved.h;
    if (saved.arrowFrom && saved.arrowTo) {
      arrowFrom.value = saved.arrowFrom;
      arrowTo.value = saved.arrowTo;
      hasArrow.value = true;
    } else {
      hasArrow.value = false;
    }
    color.value = saved.color || '#ef4444';
    labelText.value = saved.label || session.pin.title;
  } else {
    clearDrawing();
    labelText.value = session.pin.title;
  }
  toolMode.value = 'box';
  scope.value = 'full';
  capturing.value = false;
});

function clearDrawing() {
  box.x = 0;
  box.y = 0;
  box.w = 0;
  box.h = 0;
  hasArrow.value = false;
}

function onPointerDown(e: MouseEvent) {
  if (kind.value === 'none') return;
  const activeMode = kind.value === 'arrow' ? 'arrow' : kind.value === 'box' ? 'box' : toolMode.value;
  dragState.value = { mode: activeMode, startX: e.clientX, startY: e.clientY };
  if (activeMode === 'box') {
    box.x = e.clientX;
    box.y = e.clientY;
    box.w = 0;
    box.h = 0;
  } else {
    arrowFrom.value = [e.clientX, e.clientY];
    arrowTo.value = [e.clientX, e.clientY];
    hasArrow.value = true;
  }
}

function onPointerMove(e: MouseEvent) {
  const drag = dragState.value;
  if (!drag) return;
  if (drag.mode === 'box') {
    box.x = Math.min(drag.startX, e.clientX);
    box.y = Math.min(drag.startY, e.clientY);
    box.w = Math.abs(e.clientX - drag.startX);
    box.h = Math.abs(e.clientY - drag.startY);
  } else {
    arrowTo.value = [e.clientX, e.clientY];
  }
}

function onPointerUp() {
  if (dragState.value?.mode === 'box' && hasBox.value && kind.value === 'box_arrow' && !hasArrow.value) {
    // 第一次框选完成后自动建议一条指向框边沿的箭头并切换到箭头微调模式
    arrowFrom.value = [Math.max(20, box.x - 56), Math.max(40, box.y - 40)];
    arrowTo.value = [box.x + 8, box.y + 8];
    hasArrow.value = true;
    toolMode.value = 'arrow';
  }
  dragState.value = null;
}

function cancel() {
  cancelPinCapture();
}

async function confirmCapture() {
  capturing.value = true;
  try {
    await nextTick();
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));

    let region: CaptureRegion | null = null;
    if (scope.value === 'crop' && hasBox.value) {
      const pad = 16;
      region = {
        x: Math.max(0, box.x - pad),
        y: Math.max(0, box.y - pad - 24),
        w: box.w + pad * 2,
        h: box.h + pad * 2 + 24,
      };
    }

    const res = await finishPinCapture(region, {
      kind: kind.value,
      x: box.x,
      y: box.y,
      w: box.w,
      h: box.h,
      arrowFrom: hasArrow.value ? arrowFrom.value : null,
      arrowTo: hasArrow.value ? arrowTo.value : null,
      color: color.value,
      label: labelText.value.trim() || null,
    });
    if (res) {
      notifySuccess(tr('已生成桌面端原版截图'), `${res.width}x${res.height} · ${res.screenshotRel}`);
    }
  } catch (err) {
    notifyError(tr('截图失败'), String(err));
    capturing.value = false;
  }
}
</script>

<style scoped>
.capture-overlay {
  position: fixed;
  inset: 0;
  z-index: 9500;
  pointer-events: auto;
}

.capture-canvas {
  width: 100%;
  height: 100%;
  display: block;
  cursor: crosshair;
  user-select: none;
}

.capture-toolbar {
  position: fixed;
  top: 44px;
  left: 50%;
  transform: translateX(-50%);
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px;
  max-width: calc(100vw - 32px);
  padding: 8px 12px;
  border-radius: 10px;
  background: rgba(17, 24, 39, 0.92);
  color: #f9fafb;
  border: 1px solid rgba(255, 255, 255, 0.16);
  box-shadow: 0 12px 32px rgba(0, 0, 0, 0.35);
}

.toolbar-title {
  font-size: 12px;
  font-weight: 600;
  max-width: 160px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  padding-right: 4px;
}

.label-input {
  width: 120px;
}

.capture-toolbar .ui-select,
.capture-toolbar .ui-input {
  background: rgba(255, 255, 255, 0.1);
  color: #f9fafb;
  border-color: rgba(255, 255, 255, 0.22);
}

.capture-toolbar .ui-select option {
  color: #111827;
}
</style>
