// @vitest-environment happy-dom
import { computed, ref } from 'vue';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { AnalysisResultNode, RecordChatLayout, RecordColumn, RecordPropertyLayout } from '../../../lib/ipc/analysisResults';
import { useAnalysisPresentation } from './useAnalysisPresentation';

const catalog = vi.hoisted(() => ({ packages: [] as unknown[] }));
vi.mock('../../tasks/api', () => ({ usePluginCatalog: () => ({ packages: computed(() => catalog.packages) }) }));
const key = 'lovelycarve-analysis-presentation';
const column: RecordColumn = { id: 'timezone', label: '时区', kind: 'text', hidden: false, key: true, timeline: null, choices: [] };
const node = ref({ pluginId: 'p', artifactType: 'timezone', legacy: false } as AnalysisResultNode);
beforeEach(() => { localStorage.clear(); catalog.packages = []; node.value = { pluginId: 'p', artifactType: 'timezone', legacy: false } as AnalysisResultNode; });

describe('analysis reading preferences', () => {
  it('keeps the plugin default independent of row count, keyword, and page data', () => {
    const columns = ref([column]);
    const saved = ref<RecordPropertyLayout | null>({ value: 'timezone' });
    const view = useAnalysisPresentation(node, columns, saved);
    expect(view.presentation.value).toBe('properties');
    view.setPresentation('table');
    expect(view.presentation.value).toBe('table');
    expect(useAnalysisPresentation(node, columns, saved).presentation.value).toBe('table');
    node.value = { ...node.value, artifactType: 'users' };
    expect(view.presentation.value).toBe('properties');
    saved.value = null;
    expect(view.presentation.value).toBe('table');
  });

  it('can use an installed plugin hint for existing results and keeps saved hints when the plugin is absent', () => {
    catalog.packages = [{ id: 'p', records: [{ recordType: 'timezone', fields: [column], properties: { value: 'timezone' } }] }];
    const view = useAnalysisPresentation(node, ref([column]), ref(null));
    expect(view.propertyLayout.value).toEqual({ value: 'timezone' });
    const saved = useAnalysisPresentation(node, ref([column]), ref({ value: 'timezone' }));
    catalog.packages = [];
    expect(saved.propertyLayout.value).toEqual({ value: 'timezone' });
  });

  it('ignores malformed storage and keeps working when saving is unavailable', () => {
    localStorage.setItem(key, 'null');
    const view = useAnalysisPresentation(node, ref([column]), ref(null));
    expect(view.presentation.value).toBe('table');
    const storage = vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => { throw new Error('blocked'); });
    view.setPresentation('properties');
    expect(view.presentation.value).toBe('properties');
    storage.mockRestore();
  });

  it('defaults to chat presentation when ChatLayout is valid and upgrades from installed schema', () => {
    const chatColumns: RecordColumn[] = [
      { id: 'chat_id', label: '会话', kind: 'text', key: true, hidden: false, timeline: null, choices: [] },
      { id: 'content', label: '内容', kind: 'text', key: false, hidden: false, timeline: null, choices: [] },
    ];
    const chatLayout: RecordChatLayout = { threadId: 'chat_id', content: 'content' };
    node.value = { pluginId: 'p', artifactType: 'wa_msg', legacy: false } as AnalysisResultNode;
    const view = useAnalysisPresentation(node, ref(chatColumns), ref(null), ref(chatLayout));
    expect(view.canChat.value).toBe(true);
    expect(view.presentation.value).toBe('chat');
    view.setPresentation('table');
    expect(view.presentation.value).toBe('table');

    node.value = { pluginId: 'p', artifactType: 'wa_msg_old', legacy: false } as AnalysisResultNode;
    catalog.packages = [{ id: 'p', records: [{ recordType: 'wa_msg_old', fields: chatColumns, chat: chatLayout }] }];
    const upgraded = useAnalysisPresentation(node, ref(chatColumns), ref(null), ref(null));
    expect(upgraded.canChat.value).toBe(true);
    expect(upgraded.presentation.value).toBe('chat');
  });
});
