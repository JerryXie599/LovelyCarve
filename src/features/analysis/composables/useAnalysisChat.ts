import { computed, ref, shallowRef, watch, type Ref } from 'vue';
import {
  analysisChatThreads,
  analysisRecordsPage,
  type AnalysisRecord,
  type AnalysisResultNode,
  type ChatThreadSummary,
  type RecordChatLayout,
} from '../../../lib/ipc/analysisResults';

const CHAT_THREAD_LIMIT = 200;
const CHAT_MESSAGE_PAGE_SIZE = 200;

function filtersToIpc(
  filters: Record<string, string[]>,
  overrideColumn?: string,
  overrideValue?: string,
): string[] | undefined {
  const packed: string[] = [];
  for (const [column, values] of Object.entries(filters)) {
    if (overrideColumn && column === overrideColumn) continue;
    for (const value of values) packed.push(`${column}=${value}`);
  }
  if (overrideColumn && overrideValue !== undefined) {
    packed.push(`${overrideColumn}=${overrideValue}`);
  }
  return packed.length ? packed : undefined;
}

export function useAnalysisChat(options: {
  node: Ref<AnalysisResultNode | null>;
  chatLayout: Ref<RecordChatLayout | null>;
  active: Ref<boolean>;
  keyword: Ref<string>;
  filters: Ref<Record<string, string[]>>;
}) {
  const threads = shallowRef<ChatThreadSummary[]>([]);
  const threadsTotal = ref(0);
  const threadsLoading = ref(false);
  const threadsError = ref<string | null>(null);
  const threadSearch = ref('');
  const selectedThreadId = ref<string | null>(null);

  const messages = shallowRef<AnalysisRecord[]>([]);
  const messagesTotal = ref(0);
  const messagesLoading = ref(false);
  const messagesLoadingMore = ref(false);
  const messagesError = ref<string | null>(null);

  let threadRequestId = 0;
  let messageRequestId = 0;

  const filteredThreads = computed(() => {
    const q = threadSearch.value.trim().toLowerCase();
    if (!q) return threads.value;
    return threads.value.filter(thread =>
      (thread.threadTitle ?? '').toLowerCase().includes(q)
      || thread.threadId.toLowerCase().includes(q)
      || (thread.lastMessage ?? '').toLowerCase().includes(q)
    );
  });

  const selectedThread = computed(() =>
    threads.value.find(thread => thread.threadId === selectedThreadId.value) ?? null
  );

  const hasMoreMessages = computed(() => messages.value.length < messagesTotal.value);

  async function loadMessages(reset = true): Promise<void> {
    const node = options.node.value;
    const layout = options.chatLayout.value;
    const threadId = selectedThreadId.value;
    if (!options.active.value || !node || node.kind !== 'result' || !layout || !threadId) {
      if (reset) {
        messages.value = [];
        messagesTotal.value = 0;
        messagesError.value = null;
        messagesLoading.value = false;
      }
      return;
    }
    const requestId = ++messageRequestId;
    const skip = reset ? 0 : messages.value.length;
    if (reset) {
      messagesLoading.value = true;
      messagesError.value = null;
      messages.value = [];
      messagesTotal.value = 0;
    } else {
      messagesLoadingMore.value = true;
    }
    try {
      const page = await analysisRecordsPage({
        imageId: node.imageId,
        runId: node.runId,
        recordType: node.artifactType,
        legacy: node.legacy,
        keyword: options.keyword.value.trim() || undefined,
        filters: filtersToIpc(options.filters.value, layout.threadId, threadId),
        bcpScope: node.bcpScope,
        sort: layout.time ?? undefined,
        descending: false,
        skip,
        limit: CHAT_MESSAGE_PAGE_SIZE,
      });
      if (requestId !== messageRequestId) return;
      messages.value = reset ? page.items : [...messages.value, ...page.items];
      messagesTotal.value = page.total;
    } catch (error) {
      if (requestId !== messageRequestId) return;
      messagesError.value = String(error);
    } finally {
      if (requestId === messageRequestId) {
        messagesLoading.value = false;
        messagesLoadingMore.value = false;
      }
    }
  }

  async function loadThreads(): Promise<void> {
    const node = options.node.value;
    const layout = options.chatLayout.value;
    if (!options.active.value || !node || node.kind !== 'result' || !layout) {
      threadRequestId += 1;
      threads.value = [];
      threadsTotal.value = 0;
      threadsError.value = null;
      threadsLoading.value = false;
      selectedThreadId.value = null;
      messages.value = [];
      messagesTotal.value = 0;
      return;
    }
    const requestId = ++threadRequestId;
    threadsLoading.value = true;
    threadsError.value = null;
    try {
      const page = await analysisChatThreads({
        imageId: node.imageId,
        runId: node.runId,
        recordType: node.artifactType,
        legacy: node.legacy,
        keyword: options.keyword.value.trim() || undefined,
        filters: filtersToIpc(options.filters.value),
        bcpScope: node.bcpScope,
        skip: 0,
        limit: CHAT_THREAD_LIMIT,
      });
      if (requestId !== threadRequestId) return;
      threads.value = page.items;
      threadsTotal.value = page.total;
      const stillPresent = selectedThreadId.value
        && page.items.some(item => item.threadId === selectedThreadId.value);
      const nextThread = stillPresent ? selectedThreadId.value : (page.items[0]?.threadId ?? null);
      if (nextThread !== selectedThreadId.value) {
        selectedThreadId.value = nextThread;
      } else {
        void loadMessages(true);
      }
    } catch (error) {
      if (requestId !== threadRequestId) return;
      threads.value = [];
      threadsTotal.value = 0;
      threadsError.value = String(error);
    } finally {
      if (requestId === threadRequestId) threadsLoading.value = false;
    }
  }

  function selectThread(threadId: string) {
    if (selectedThreadId.value === threadId) return;
    selectedThreadId.value = threadId;
  }

  function loadMoreMessages() {
    if (!hasMoreMessages.value || messagesLoading.value || messagesLoadingMore.value) return;
    void loadMessages(false);
  }

  watch(
    () => [
      options.active.value,
      options.node.value?.runId,
      options.node.value?.artifactType,
      options.chatLayout.value?.threadId,
      options.keyword.value,
      JSON.stringify(options.filters.value),
    ],
    () => {
      void loadThreads();
    },
    { immediate: true },
  );

  watch(selectedThreadId, () => {
    void loadMessages(true);
  });

  return {
    threads,
    filteredThreads,
    threadsTotal,
    threadsLoading,
    threadsError,
    threadSearch,
    selectedThreadId,
    selectedThread,
    messages,
    messagesTotal,
    messagesLoading,
    messagesLoadingMore,
    messagesError,
    hasMoreMessages,
    selectThread,
    loadMoreMessages,
    retryThreads: loadThreads,
    retryMessages: () => loadMessages(true),
  };
}
