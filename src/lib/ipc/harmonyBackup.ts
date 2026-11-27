import { invokeCmd } from './client';
import type { ImageInfo } from './image';
import type { ImageId } from './ids';

export function harmonyBackupUnlock(args: {
  imageId: ImageId;
  password: string;
  taskId: string;
}): Promise<ImageInfo> {
  return invokeCmd<ImageInfo>('harmony_backup_unlock', args);
}
