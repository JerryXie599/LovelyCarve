import { tr } from '../../../composables/useLocale';
import { onBeforeUnmount, ref, watch } from 'vue';
import type { ImageInfo } from '../../../lib/ipc/image';
import { imageUnload } from '../../../lib/ipc/image';
import { harmonyBackupUnlock } from '../../../lib/ipc/harmonyBackup';
import { taskCancel } from '../../../lib/ipc/taskRuntime';
import { notifyError } from '../../../composables/useToast';

export function useHarmonyBackup(info: () => ImageInfo, openImage: (path: string) => void) {
  const password = ref('');
  const error = ref('');
  const busy = ref(false);
  const cancelling = ref(false);
  let generation = 0;
  let taskId: string | undefined;

  watch(() => info().id, () => {
    generation += 1;
    password.value = '';
    error.value = '';
    busy.value = false;
    cancelling.value = false;
    taskId = undefined;
  });

  async function unlock() {
    if (busy.value) return;
    const captured = generation;
    const imageId = info().id;
    const currentTaskId = crypto.randomUUID();
    taskId = currentTaskId;
    busy.value = true;
    error.value = '';
    cancelling.value = false;
    const request = { imageId, password: password.value, taskId: currentTaskId };
    password.value = '';
    try {
      const image = await harmonyBackupUnlock(request);
      if (captured === generation && !cancelling.value) openImage(image.path);
      else await imageUnload({ imageId: image.id });
    } catch (cause) {
      if (captured === generation) error.value = String(cause);
      else notifyError(tr("HarmonyOS NEXT 备份解锁失败"), String(cause));
    } finally {
      request.password = '';
      if (captured === generation) {
        busy.value = false;
        taskId = undefined;
      }
    }
  }

  async function cancel() {
    if (!taskId) return;
    const captured = generation;
    cancelling.value = true;
    try {
      await taskCancel({ taskId });
    } catch (cause) {
      if (captured === generation) {
        error.value = String(cause);
        cancelling.value = false;
      }
    }
  }

  onBeforeUnmount(() => {
    generation += 1;
    password.value = '';
  });

  return { password, error, busy, cancelling, unlock, cancel };
}
