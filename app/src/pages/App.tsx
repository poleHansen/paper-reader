import { useEffect, useState } from 'react';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import {
  confirmPaperMetadata,
  deleteModelConfig,
  getAgentRun,
  getModelConfigDetail,
  getRecentModelConfig,
  getPaperParseStatus,
  getProfile,
  getReaderSnapshot,
  importPaperFromFile,
  importPaperFromLink,
  listModelConfigs,
  listLibraryItems,
  pickPdfFile,
  runAgent,
  saveModelConfig,
  selectModelConfig,
  updateModelConfig,
  updateLibraryItem,
  searchPapers,
  testModelConnection,
  upsertProfile,
} from '../services/commands';
import type {
  AgentRunDetail,
  ConfirmPaperMetadataResponse,
  EvidenceItem,
  LibraryItem,
  ModelConnectionResult,
  ModelConfigDetailResponse,
  ModelConfigListResponse,
  ModelConfigResponse,
  ModelConfigRequest,
  PaperSearchResult,
  ReaderSnapshot,
  UserProfile,
} from '../types/contracts';

type View = 'dashboard' | 'onboarding' | 'search' | 'upload' | 'reader' | 'library' | 'model';

const navigationItems: Array<{ view: View; label: string }> = [
  { view: 'dashboard', label: 'Dashboard' },
  { view: 'search', label: 'Search' },
  { view: 'upload', label: 'Upload' },
  { view: 'reader', label: 'Reader' },
  { view: 'library', label: 'Library' },
  { view: 'model', label: 'Model Settings' },
];

const defaultProfile: UserProfile = {
  role: 'graduate_student',
  researchField: 'llm_agents',
  focusTopic: 'agent memory',
  readingGoal: 'deep_understanding',
  outputLanguage: 'zh-CN',
  experienceLevel: 'intermediate',
};

export function App() {
  const [activeView, setActiveView] = useState<View>('dashboard');
  const [query, setQuery] = useState('agent memory');
  const [searchResults, setSearchResults] = useState<PaperSearchResult[]>([]);
  const [libraryItems, setLibraryItems] = useState<LibraryItem[]>([]);
  const [profile, setProfile] = useState<UserProfile>(defaultProfile);
  const [statusText, setStatusText] = useState('Ready');
  const [modelStatus, setModelStatus] = useState<ModelConnectionResult | null>(null);
  const [savedModels, setSavedModels] = useState<ModelConfigResponse[]>([]);
  const [selectedModelId, setSelectedModelId] = useState('');
  const [showApiKey, setShowApiKey] = useState(false);
  const [modelDraft, setModelDraft] = useState<ModelConfigRequest>({
    displayName: 'Default OpenAI-compatible',
    provider: 'openai_compatible',
    baseUrl: 'https://api.openai.com/v1',
    modelName: 'gpt-4.1-mini',
    apiKey: '',
    apiType: 'chat_completions',
    agentType: null,
    isDefault: true,
  });
  const [filePath, setFilePath] = useState('');
  const [paperUrl, setPaperUrl] = useState('');
  const [selectedPaperId, setSelectedPaperId] = useState<string | null>(null);
  const [readerSnapshot, setReaderSnapshot] = useState<ReaderSnapshot | null>(null);
  const [hasProfile, setHasProfile] = useState(false);
  const [libraryFilterStatus, setLibraryFilterStatus] = useState('');
  const [libraryKeyword, setLibraryKeyword] = useState('');
  const [metadataDraft, setMetadataDraft] = useState({
    paperId: '',
    title: '',
    authors: '',
    year: '',
    venue: '',
    abstract: '',
  });
  const [activeRun, setActiveRun] = useState<AgentRunDetail | null>(null);
  const libraryHighlights = libraryItems.slice(0, 3);
  const workflowSteps = buildWorkflowSteps(readerSnapshot?.workflowCurrentStep ?? null);

  useEffect(() => {
    void loadProfile();
    void refreshLibrary();
    void loadSavedModels();
  }, []);

  useEffect(() => {
    if (!selectedPaperId || activeView !== 'reader') {
      return;
    }

    const timer = window.setInterval(() => {
      void handleRefreshParseStatus();
    }, 4000);

    return () => window.clearInterval(timer);
  }, [selectedPaperId, activeView]);

  useEffect(() => {
    let unlisten: UnlistenFn | null = null;

    void listen<ParseStatusEvent>('paper-parse-status-changed', async (event) => {
      const payload = event.payload;
      if (!payload || !selectedPaperId || payload.paperId !== selectedPaperId) {
        return;
      }

      setStatusText(`Parse status: ${payload.parseStatus} (${payload.stage})`);

      if (payload.parseStatus === 'succeeded' || payload.parseStatus === 'failed' || payload.progress >= 40) {
        await loadReaderSnapshot(payload.paperId);
      }
    }).then((dispose) => {
      unlisten = dispose;
    }).catch((error) => {
      setStatusText(formatError(error));
    });

    return () => {
      if (unlisten) {
        void unlisten();
      }
    };
  }, [selectedPaperId]);

  async function loadProfile() {
    try {
      const saved = await getProfile();
      setProfile(saved);
      setHasProfile(true);
    } catch (error) {
      if (isNotFoundError(error)) {
        setHasProfile(false);
        setActiveView('onboarding');
        return;
      }
      setStatusText(formatError(error));
    }
  }

  async function refreshLibrary(status = libraryFilterStatus, keyword = libraryKeyword) {
    try {
      const items = await listLibraryItems({ page: 1, pageSize: 20, status: status || undefined, keyword: keyword || undefined });
      setLibraryItems(items.items);
    } catch (error) {
      setStatusText(formatError(error));
    }
  }

  async function handleSearch() {
    setStatusText('Searching arXiv...');
    try {
      const response = await searchPapers({ query, source: 'arxiv', page: 1, pageSize: 10 });
      setSearchResults(response.items);
      setStatusText(`Loaded ${response.items.length} papers from ${response.source}`);
    } catch (error) {
      setStatusText(formatError(error));
    }
  }

  async function handleSaveProfile() {
    try {
      const saved = await upsertProfile(profile);
      setProfile(saved);
      setHasProfile(true);
      setActiveView('search');
      setStatusText('Profile saved');
    } catch (error) {
      setStatusText(formatError(error));
    }
  }

  async function handleSaveModel() {
    try {
      const saved = selectedModelId
        ? await updateModelConfig({ id: selectedModelId, ...modelDraft })
        : await saveModelConfig(modelDraft);
      const detail = await getModelConfigDetail({ id: saved.id });
      applySavedModel(detail);
      await loadSavedModels(saved.id);
      setShowApiKey(false);
      setStatusText(`${selectedModelId ? 'Model preset updated' : 'Model preset saved'}: ${saved.displayName}`);
    } catch (error) {
      setStatusText(formatError(error));
    }
  }

  async function loadSavedModels(preferredId?: string) {
    try {
      const [listResponse, recentResponse] = await Promise.allSettled([listModelConfigs(), getRecentModelConfig()]);
      const list = listResponse.status === 'fulfilled' ? listResponse.value : ({ items: [], recentId: null } satisfies ModelConfigListResponse);
      const recent = recentResponse.status === 'fulfilled' ? recentResponse.value : null;
      setSavedModels(list.items);

      const targetId = preferredId ?? recent?.id ?? list.recentId ?? list.items[0]?.id ?? '';
      setSelectedModelId(targetId);

      const targetModel = list.items.find((item) => item.id === targetId) ?? recent;
      if (targetModel) {
        const detail = await getModelConfigDetail({ id: targetModel.id });
        applySavedModel(detail);
      }
    } catch (error) {
      setStatusText(formatError(error));
    }
  }

  function applySavedModel(model: ModelConfigDetailResponse) {
    setModelDraft({
      displayName: model.displayName,
      provider: model.provider,
      baseUrl: model.baseUrl,
      modelName: model.modelName,
      apiKey: model.apiKey,
      apiType: model.apiType,
      agentType: model.agentType,
      isDefault: model.isDefault,
    });
    setSelectedModelId(model.id);
    setShowApiKey(false);
  }

  async function handleSelectSavedModel(modelId: string) {
    if (!modelId) {
      setSelectedModelId('');
      setModelDraft({
        displayName: 'New OpenAI-compatible preset',
        provider: 'openai_compatible',
        baseUrl: 'https://api.openai.com/v1',
        modelName: 'gpt-4.1-mini',
        apiKey: '',
        apiType: 'chat_completions',
        agentType: null,
        isDefault: true,
      });
      return;
    }

    try {
      const selected = await selectModelConfig({ id: modelId });
      const detail = await getModelConfigDetail({ id: selected.id });
      applySavedModel(detail);
      await loadSavedModels(selected.id);
      setStatusText(`Selected model preset: ${selected.displayName}`);
    } catch (error) {
      setStatusText(formatError(error));
    }
  }

  async function handleDeleteSelectedModel() {
    if (!selectedModelId) {
      setStatusText('Select a saved preset to delete');
      return;
    }

    try {
      const deletingName = savedModels.find((item) => item.id === selectedModelId)?.displayName ?? 'preset';
      const confirmed = window.confirm(`Delete model preset \"${deletingName}\"? This cannot be undone.`);
      if (!confirmed) {
        setStatusText('Preset deletion canceled');
        return;
      }

      await deleteModelConfig({ id: selectedModelId });
      setSelectedModelId('');
      setModelStatus(null);
      await loadSavedModels();
      setStatusText(`Deleted model preset: ${deletingName}`);
    } catch (error) {
      setStatusText(formatError(error));
    }
  }

  async function handleTestModel() {
    try {
      const result = await testModelConnection({
        provider: modelDraft.provider,
        baseUrl: modelDraft.baseUrl,
        modelName: modelDraft.modelName,
        apiKey: modelDraft.apiKey,
        apiType: modelDraft.apiType,
      });
      setModelStatus(result);
      setStatusText(result.connected ? 'Model request succeeded' : result.statusText);
    } catch (error) {
      setStatusText(formatError(error));
    }
  }

  async function handleImportFile() {
    if (!filePath.trim()) {
      setStatusText('Select a PDF file or enter a file path first');
      return;
    }

    try {
      const result = await importPaperFromFile({ filePath });
      setSelectedPaperId(result.paperId);
      setMetadataDraft((current) => ({ ...current, paperId: result.paperId, title: getFileNameFromPath(filePath) }));
      setStatusText(`Imported ${result.paperId}`);
      await loadReaderSnapshot(result.paperId);
      await refreshLibrary();
      setActiveView('reader');
    } catch (error) {
      setStatusText(formatError(error));
    }
  }

  async function handlePickPdfFile() {
    try {
      const selectedPath = await pickPdfFile();
      if (!selectedPath) {
        setStatusText('File selection canceled');
        return;
      }

      setFilePath(selectedPath);
      setStatusText('PDF selected and ready to import');
    } catch (error) {
      setStatusText(formatError(error));
    }
  }

  function handleClearSelectedFile() {
    setFilePath('');
    setStatusText('Selected file cleared');
  }

  async function handleImportLink() {
    try {
      const result = await importPaperFromLink({ url: paperUrl });
      setSelectedPaperId(result.paperId);
      setMetadataDraft((current) => ({ ...current, paperId: result.paperId, title: paperUrl }));
      setStatusText(`Imported ${result.paperId} from link`);
      await loadReaderSnapshot(result.paperId);
      await refreshLibrary();
      setActiveView(result.metadataNeedsConfirmation ? 'upload' : 'reader');
    } catch (error) {
      setStatusText(formatError(error));
    }
  }

  async function loadReaderSnapshot(paperId: string) {
    try {
      const snapshot = await getReaderSnapshot({ paperId });
      setReaderSnapshot(snapshot);
      if (snapshot.activeRun) {
        setStatusText(`${snapshot.activeRun.agentType} is running, batch ${snapshot.activeRun.currentBatchIndex}/${snapshot.activeRun.currentBatchCount}`);
      }
      if (activeRun && activeRun.paperId !== paperId) {
        setActiveRun(null);
      }
    } catch (error) {
      setStatusText(formatError(error));
    }
  }

  async function handleOpenLibraryItem(item: LibraryItem) {
    setSelectedPaperId(item.paperId);
    setActiveRun(null);
    setStatusText(`Opening ${item.title}`);
    await loadReaderSnapshot(item.paperId);
    setActiveView('reader');
  }

  async function handleRefreshParseStatus() {
    if (!selectedPaperId) {
      return;
    }

    try {
      const status = await getPaperParseStatus({ paperId: selectedPaperId });
      setStatusText(`Parse status: ${status.parseStatus} (${status.stage})`);
      await loadReaderSnapshot(selectedPaperId);
    } catch (error) {
      setStatusText(formatError(error));
    }
  }

  async function handleRunAgent(agentType: string) {
    if (!selectedPaperId) {
      setStatusText('Open a paper before running an agent');
      return;
    }

    try {
      const run = await runAgent({
        paperId: selectedPaperId,
        agentType,
        userQuestion: `Please run ${agentType} for the current paper and keep the output grounded in available evidence.`,
        force: false,
      });
      setStatusText(`${agentType} started`);
      const detail = await waitForAgentRun(run.runId);
      setActiveRun(detail);
      setStatusText(`${agentType} finished with status ${detail.status}`);
      await loadReaderSnapshot(selectedPaperId);
    } catch (error) {
      setStatusText(formatError(error));
    }
  }

  async function handleConfirmMetadata() {
    if (!metadataDraft.paperId) {
      setStatusText('No paper selected for metadata confirmation');
      return;
    }

    try {
      const response: ConfirmPaperMetadataResponse = await confirmPaperMetadata({
        paperId: metadataDraft.paperId,
        title: metadataDraft.title,
        authors: metadataDraft.authors.split(',').map((value) => value.trim()).filter(Boolean),
        year: metadataDraft.year ? Number(metadataDraft.year) : null,
        venue: metadataDraft.venue || null,
        abstract: metadataDraft.abstract || null,
      });
      setStatusText(`Metadata confirmed for ${response.title}`);
      await loadReaderSnapshot(response.paperId);
      await refreshLibrary();
      setActiveView('reader');
    } catch (error) {
      setStatusText(formatError(error));
    }
  }

  async function handleApplyLibraryFilters() {
    await refreshLibrary(libraryFilterStatus, libraryKeyword);
  }

  async function handleCycleLibraryStatus(item: LibraryItem) {
    const nextStatus = item.status === 'queued' ? 'reading' : item.status === 'reading' ? 'completed' : item.status === 'completed' ? 'archived' : 'queued';
    try {
      await updateLibraryItem({ id: item.id, status: nextStatus });
      setStatusText(`Updated ${item.title} to ${nextStatus}`);
      await refreshLibrary();
      if (selectedPaperId === item.paperId) {
        await loadReaderSnapshot(item.paperId);
      }
    } catch (error) {
      setStatusText(formatError(error));
    }
  }

  async function handleToggleStar(item: LibraryItem) {
    try {
      await updateLibraryItem({ id: item.id, starred: !item.starred });
      setStatusText(item.starred ? `Removed star from ${item.title}` : `Starred ${item.title}`);
      await refreshLibrary();
      if (selectedPaperId === item.paperId) {
        await loadReaderSnapshot(item.paperId);
      }
    } catch (error) {
      setStatusText(formatError(error));
    }
  }

  function handleSelectSearchResult(item: PaperSearchResult) {
    setPaperUrl(item.pdfUrl ?? item.detailUrl);
    setStatusText(`Prepared ${item.title} for import`);
    setActiveView('upload');
  }

  function handleSkipOnboarding() {
    setHasProfile(false);
    setStatusText('Profile skipped. Generic mode will be used.');
    setActiveView('dashboard');
  }

  return (
    <div className="shell">
      <aside className="sidebar">
        <div className="brandBlock">
          <span className="eyebrow">Windows research workstation</span>
          <h1>Paper Reader</h1>
          <p>From search to import to structured reading, in one runtime.</p>
        </div>

        <nav className="navList">
          {navigationItems.map((item) => (
            <button
              key={item.view}
              className={item.view === activeView ? 'navItem navItemActive' : 'navItem'}
              onClick={() => setActiveView(item.view)}
            >
              {item.label}
            </button>
          ))}
        </nav>

        <div className="status">
          <strong>Runtime status</strong>
          <span>{statusText}</span>
        </div>

        <div className="sidebarMeta">
          <span>Profile: {hasProfile ? 'configured' : 'generic mode'}</span>
          <span>Library items: {libraryItems.length}</span>
          <span>Reader ready: {selectedPaperId ? 'yes' : 'no'}</span>
        </div>
      </aside>
      <main className="content">
        {activeView === 'dashboard' ? (
          <>
            <section className="hero card">
              <div>
                <span className="eyebrow">MVP workspace</span>
                <h2>Research dashboard</h2>
                <p className="muted">Start from search, upload a local PDF, or resume reading from the library.</p>
              </div>
              <div className="heroActions">
                <button onClick={() => setActiveView('search')}>Search papers</button>
                <button className="secondaryButton" onClick={() => setActiveView('upload')}>Import PDF</button>
              </div>
            </section>

            <section className="grid gridThree">
              <section className="card statCard">
                <span className="eyebrow">Profile</span>
                <strong>{hasProfile ? profile.role : 'Generic mode'}</strong>
                <span>{hasProfile ? profile.researchField : 'No personalized profile yet'}</span>
              </section>
              <section className="card statCard">
                <span className="eyebrow">Library</span>
                <strong>{libraryItems.length}</strong>
                <span>Tracked papers in the local shelf</span>
              </section>
              <section className="card statCard">
                <span className="eyebrow">Reader</span>
                <strong>{readerSnapshot?.parseStatus ?? 'idle'}</strong>
                <span>{readerSnapshot?.title ?? 'No paper opened yet'}</span>
              </section>
            </section>

            <section className="grid">
              <section className="card">
                <div className="sectionHeader">
                  <div>
                    <h2>Next actions</h2>
                    <p className="muted">Keep the MVP flow aligned with the product docs.</p>
                  </div>
                </div>
                <div className="list compactList">
                  <article className="listItem interactive" onClick={() => setActiveView(hasProfile ? 'search' : 'onboarding')}>
                    <strong>{hasProfile ? 'Search and pick a paper' : 'Complete onboarding'}</strong>
                    <span>{hasProfile ? 'Run arXiv search and jump into import or reading.' : 'Save role, field, and reading goal before starting.'}</span>
                  </article>
                  <article className="listItem interactive" onClick={() => setActiveView('upload')}>
                    <strong>Import local PDF or paper link</strong>
                    <span>Bring a paper into the parse pipeline and the library.</span>
                  </article>
                  <article className="listItem interactive" onClick={() => setActiveView('library')}>
                    <strong>Resume from library</strong>
                    <span>Open a tracked paper and continue from the Reader.</span>
                  </article>
                </div>
              </section>

              <section className="card">
                <h2>Recent library items</h2>
                <div className="list compactList">
                  {libraryHighlights.length > 0 ? (
                    libraryHighlights.map((item) => (
                      <article className="listItem interactive" key={item.id} onClick={() => void handleOpenLibraryItem(item)}>
                        <strong>{item.title}</strong>
                        <span>Status: {item.status}</span>
                      </article>
                    ))
                  ) : (
                    <div className="emptyState compactEmpty">
                      <strong>No papers yet</strong>
                      <span>Use Upload or Search to seed the library.</span>
                    </div>
                  )}
                </div>
              </section>
            </section>
          </>
        ) : null}

        {activeView === 'onboarding' ? (
          <section className="card pageCard pageCardReader">
            <div className="sectionHeader">
              <div>
                <span className="eyebrow">First run</span>
                <h2>User profile</h2>
                <p className="muted">This drives personalized reading and output tone. You can also skip and stay in generic mode.</p>
              </div>
              <button className="secondaryButton" onClick={handleSkipOnboarding}>Skip for now</button>
            </div>
            <div className="formGrid">
              <input value={profile.role} onChange={(event) => setProfile({ ...profile, role: event.target.value })} placeholder="role" />
              <input value={profile.researchField} onChange={(event) => setProfile({ ...profile, researchField: event.target.value })} placeholder="research field" />
              <input value={profile.focusTopic ?? ''} onChange={(event) => setProfile({ ...profile, focusTopic: event.target.value })} placeholder="focus topic" />
              <input value={profile.readingGoal} onChange={(event) => setProfile({ ...profile, readingGoal: event.target.value })} placeholder="reading goal" />
            </div>
            <div className="row rowEnd">
              <button onClick={() => void handleSaveProfile()}>Save profile</button>
            </div>
          </section>
        ) : null}

        {activeView === 'search' ? (
          <section className="card pageCard pageCardReader">
            <div className="sectionHeader">
              <div>
                <span className="eyebrow">Search</span>
                <h2>Find papers from arXiv</h2>
                <p className="muted">Search results can be staged into the upload flow, then moved into Reader.</p>
              </div>
            </div>
            <div className="row">
              <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="agent memory, retrieval augmented reasoning..." />
              <button onClick={() => void handleSearch()}>Search arXiv</button>
            </div>
            <div className="list">
              {searchResults.length > 0 ? (
                searchResults.map((item) => (
                  <article className="listItem" key={item.id}>
                    <strong>{item.title}</strong>
                    <span>{item.authors.join(', ') || 'Unknown authors'}</span>
                    <span>{item.venue} · {item.year ?? 'unknown'}</span>
                    <div className="row rowWrap">
                      <button className="secondaryButton" onClick={() => handleSelectSearchResult(item)}>Import this paper</button>
                      <a href={item.detailUrl} target="_blank" rel="noreferrer">Open source page</a>
                    </div>
                  </article>
                ))
              ) : (
                <div className="emptyState">
                  <strong>Search is ready</strong>
                  <span>Run a query to load papers from arXiv.</span>
                </div>
              )}
            </div>
          </section>
        ) : null}

        {activeView === 'upload' ? (
          <section className="grid uploadLayout">
            <section className="card pageCard">
              <span className="eyebrow">Upload</span>
              <h2>Import local PDF</h2>
              <p className="muted">Direct file import creates a local paper record and a queued parse state.</p>
              <input value={filePath} onChange={(event) => setFilePath(event.target.value)} placeholder="C:/path/to/paper.pdf" />
              {filePath ? (
                <div className="selectedFileCard">
                  <strong>{getFileNameFromPath(filePath)}</strong>
                  <span>{filePath}</span>
                </div>
              ) : (
                <div className="selectedFileCard selectedFileCardEmpty">
                  <strong>No PDF selected</strong>
                  <span>Choose a local PDF with the system dialog or paste a path manually.</span>
                </div>
              )}
              <div className="row rowWrap">
                <button className="secondaryButton" onClick={() => void handlePickPdfFile()}>Choose PDF</button>
                <button className="secondaryButton" onClick={handleClearSelectedFile} disabled={!filePath}>Clear</button>
                <button onClick={() => void handleImportFile()}>Import PDF</button>
              </div>
            </section>

            <section className="card pageCard">
              <span className="eyebrow">Link import</span>
              <h2>Import by URL</h2>
              <p className="muted">Paste a direct PDF link or use a search result as a starting point.</p>
              <input value={paperUrl} onChange={(event) => setPaperUrl(event.target.value)} placeholder="https://example.org/paper.pdf" />
              <button onClick={() => void handleImportLink()}>Import from link</button>
            </section>

            <section className="card pageCard uploadWideCard">
              <span className="eyebrow">Metadata confirmation</span>
              <h2>Confirm paper metadata</h2>
              <p className="muted">Use this after link import or local upload to replace placeholder titles and seed Reader context.</p>
              <div className="formGrid">
                <input value={metadataDraft.paperId} onChange={(event) => setMetadataDraft({ ...metadataDraft, paperId: event.target.value })} placeholder="paper id" />
                <input value={metadataDraft.title} onChange={(event) => setMetadataDraft({ ...metadataDraft, title: event.target.value })} placeholder="title" />
                <input value={metadataDraft.authors} onChange={(event) => setMetadataDraft({ ...metadataDraft, authors: event.target.value })} placeholder="authors, comma separated" />
                <input value={metadataDraft.year} onChange={(event) => setMetadataDraft({ ...metadataDraft, year: event.target.value })} placeholder="year" />
                <input value={metadataDraft.venue} onChange={(event) => setMetadataDraft({ ...metadataDraft, venue: event.target.value })} placeholder="venue" />
                <input value={metadataDraft.abstract} onChange={(event) => setMetadataDraft({ ...metadataDraft, abstract: event.target.value })} placeholder="abstract" />
              </div>
              <div className="row rowEnd">
                <button className="secondaryButton" onClick={() => void handleConfirmMetadata()}>Confirm metadata</button>
              </div>
            </section>
          </section>
        ) : null}

        {activeView === 'reader' ? (
          <section className="card pageCard">
            <div className="sectionHeader">
              <div>
                <span className="eyebrow">Reader</span>
                <h2>Paper workspace</h2>
                <p className="muted">Current implementation is a minimal snapshot view. It now behaves like a real destination in the flow.</p>
              </div>
              <button onClick={() => void handleRefreshParseStatus()} disabled={!selectedPaperId}>Refresh status</button>
            </div>
            {readerSnapshot ? (
              <div className="readerGrid">
                <section className="readerPanel">
                  <span className="eyebrow">Document</span>
                  <strong>{readerSnapshot.title}</strong>
                  <span>{readerSnapshot.authors.length > 0 ? readerSnapshot.authors.join(', ') : 'Authors unavailable'}</span>
                  <span>{readerSnapshot.venue ?? 'Unknown venue'} · {readerSnapshot.year ?? 'unknown year'}</span>
                  <span>Source: {readerSnapshot.source}</span>
                  <span>File: {readerSnapshot.fileName ?? 'No local file recorded'}</span>
                  <span>MIME: {readerSnapshot.mimeType ?? 'unknown'}</span>
                  <span>Size: {readerSnapshot.sizeBytes ?? 0} bytes</span>
                </section>

                <section className="readerPanel readerPanelScrollable">
                  <span className="eyebrow">Workflow</span>
                  <strong>Parse: {readerSnapshot.parseStatus}</strong>
                  <span>Progress: {readerSnapshot.parseProgress}%</span>
                  <span>Current step: {readerSnapshot.workflowCurrentStep}</span>
                  <span>Next action: {readerSnapshot.nextActionRequired ?? 'none'}</span>
                  <span>Library status: {readerSnapshot.libraryStatus ?? 'not saved'}</span>
                  <span>Starred: {readerSnapshot.starred ? 'yes' : 'no'}</span>
                  <span>Allowed actions: {readerSnapshot.allowedActions.join(', ') || 'none'}</span>
                  <div className="workflowTimeline">
                    {workflowSteps.map((step) => (
                      <div key={step.id} className={`timelineStep ${step.state}`}>
                        <strong>{step.label}</strong>
                        <span>{step.caption}</span>
                      </div>
                    ))}
                  </div>
                  <div className="actionStack">
                    {readerSnapshot.allowedActions.includes('run_quick_read') ? <button onClick={() => void handleRunAgent('quick_read')}>Run quick read</button> : null}
                    {readerSnapshot.allowedActions.includes('run_careful_read') ? <button onClick={() => void handleRunAgent('careful_read')}>Run careful read</button> : null}
                    {readerSnapshot.allowedActions.includes('run_deep_read') ? <button onClick={() => void handleRunAgent('deep_read')}>Run deep read</button> : null}
                    {readerSnapshot.allowedActions.includes('run_summary') ? <button onClick={() => void handleRunAgent('summary')}>Run summary</button> : null}
                  </div>
                  {readerSnapshot.parseErrorMessage ? <span>Error: {readerSnapshot.parseErrorMessage}</span> : null}
                </section>

                <section className="readerPanel readerPanelScrollable readerPanelAnalysis">
                  <span className="eyebrow">Analysis</span>
                  <p>{readerSnapshot.abstractText ?? 'No abstract available yet. This paper is waiting for the parse pipeline and agent runtime.'}</p>
                  <span>Tags: {readerSnapshot.libraryTags.length > 0 ? readerSnapshot.libraryTags.join(', ') : 'none'}</span>
                  <span>Latest handoff ids: {readerSnapshot.latestHandoffSummaryIds.join(', ') || 'none yet'}</span>
                  <span>Fallback actions: {readerSnapshot.fallbackActions.join(', ') || 'none'}</span>
                  {readerSnapshot.activeRun ? (
                    <div className="detailGroup">
                      <span className="detailLabel">Active batch run</span>
                      <span>{readerSnapshot.activeRun.agentType} · 第 {readerSnapshot.activeRun.currentBatchIndex} / {readerSnapshot.activeRun.currentBatchCount} 批分析中</span>
                      <span>Sections: {readerSnapshot.activeRun.contextPlan.selectedSectionIds.join(', ') || 'none'}</span>
                      <span>Mode: {readerSnapshot.activeRun.contextPlan.runtimeMode} / {readerSnapshot.activeRun.contextPlan.sectionStrategy}</span>
                    </div>
                  ) : null}
                  {readerSnapshot.latestAgentRuns.length > 0 ? (
                    <div className="runList">
                      {readerSnapshot.latestAgentRuns.map((run) => (
                        <button
                          key={run.id}
                          className="runListItem secondaryButton"
                          onClick={() => {
                            void getAgentRun({ runId: run.id })
                              .then(setActiveRun)
                              .catch((error) => setStatusText(formatError(error)));
                          }}
                        >
                          <strong>{run.agentType}</strong>
                          <span>{run.status}</span>
                          <span>{run.contextPlan ? `sections ${run.contextPlan.selectedSectionIds.length}, batches ${run.contextPlan.batchCount}` : 'no context plan'}</span>
                          <span>{run.summary ?? 'No summary yet'}</span>
                        </button>
                      ))}
                    </div>
                  ) : null}
                  {activeRun ? (
                    <div className="runCard">
                      <strong>{activeRun.agentType}</strong>
                      <span>Status: {activeRun.status}</span>
                      <span>Finished: {activeRun.finishedAt ?? 'n/a'}</span>
                      {activeRun.contextPlan ? (
                        <div className="detailGroup">
                          <span className="detailLabel">Context plan</span>
                          <span>Mode: {activeRun.contextPlan.runtimeMode}</span>
                          <span>Strategy: {activeRun.contextPlan.sectionStrategy}</span>
                          <span>Sections: {activeRun.contextPlan.selectedSectionIds.join(', ') || 'none'}</span>
                          <span>Batch progress: {activeRun.contextPlan.currentBatchIndex} / {activeRun.contextPlan.batchCount}</span>
                        </div>
                      ) : null}
                      {activeRun.status === 'failed' ? (
                        <div className="detailGroup">
                          <span className="detailLabel">Failure details</span>
                          <span>Code: {activeRun.errorCode ?? 'UNKNOWN_ERROR'}</span>
                          <p>{formatRunErrorMessage(activeRun.errorMessage)}</p>
                        </div>
                      ) : null}
                      <div className="jsonPreview">
                        {renderRunSnapshot(activeRun.outputSnapshot)}
                      </div>
                      {activeRun.handoffSummary ? (
                        <div className="handoffCard">
                          <strong>{activeRun.handoffSummary.stage}</strong>
                          <p>{activeRun.handoffSummary.compressedConclusion}</p>
                          {activeRun.handoffSummary.keyPoints.length > 0 ? (
                            <div className="detailGroup">
                              <span className="detailLabel">Key points</span>
                              <ul className="detailList">
                                {activeRun.handoffSummary.keyPoints.map((item) => (
                                  <li key={item}>{item}</li>
                                ))}
                              </ul>
                            </div>
                          ) : null}
                          {activeRun.handoffSummary.carryForwardQuestions.length > 0 ? (
                            <div className="detailGroup">
                              <span className="detailLabel">Carry-forward questions</span>
                              <ul className="detailList">
                                {activeRun.handoffSummary.carryForwardQuestions.map((item) => (
                                  <li key={item}>{item}</li>
                                ))}
                              </ul>
                            </div>
                          ) : null}
                          {activeRun.handoffSummary.carryForwardEvidence.length > 0 ? (
                            <div className="detailGroup">
                              <span className="detailLabel">Carry-forward evidence</span>
                              <div className="evidenceList">
                                {activeRun.handoffSummary.carryForwardEvidence.map((item, index) => (
                                  <article className="evidenceCard" key={`${item.locator}-${index}`}>
                                    <strong>{item.section}</strong>
                                    <p>{item.quote}</p>
                                    <span>{formatEvidenceMeta(item)}</span>
                                  </article>
                                ))}
                              </div>
                            </div>
                          ) : null}
                          <span>Next: {activeRun.handoffSummary.nextStepSuggestion}</span>
                        </div>
                      ) : null}
                    </div>
                  ) : null}
                </section>
              </div>
            ) : (
              <div className="emptyState">
                <strong>Reader is waiting for a paper</strong>
                <span>Import a PDF or open an item from the library to load the snapshot view.</span>
              </div>
            )}
          </section>
        ) : null}

        {activeView === 'library' ? (
          <section className="card pageCard">
            <div className="sectionHeader">
              <div>
                <span className="eyebrow">Library</span>
                <h2>Tracked papers</h2>
                <p className="muted">Open any entry to jump back into Reader.</p>
              </div>
            </div>
            <div className="row rowWrap">
              <select value={libraryFilterStatus} onChange={(event) => setLibraryFilterStatus(event.target.value)}>
                <option value="">All status</option>
                <option value="queued">Queued</option>
                <option value="reading">Reading</option>
                <option value="completed">Completed</option>
                <option value="archived">Archived</option>
              </select>
              <input value={libraryKeyword} onChange={(event) => setLibraryKeyword(event.target.value)} placeholder="Search title or abstract" />
              <button className="secondaryButton" onClick={() => void handleApplyLibraryFilters()}>Apply filters</button>
            </div>
            <div className="list">
              {libraryItems.length > 0 ? (
                libraryItems.map((item) => (
                  <article className="listItem" key={item.id}>
                    <strong className="interactive" onClick={() => void handleOpenLibraryItem(item)}>{item.title}</strong>
                    <span>Status: {item.status}</span>
                    <span>Updated: {item.updatedAt}</span>
                    <span>Tags: {item.tags.join(', ') || 'none'}</span>
                    <div className="row rowWrap">
                      <button className="secondaryButton" onClick={() => void handleCycleLibraryStatus(item)}>Cycle status</button>
                      <button className="secondaryButton" onClick={() => void handleToggleStar(item)}>{item.starred ? 'Unstar' : 'Star'}</button>
                      <button onClick={() => void handleOpenLibraryItem(item)}>Open in Reader</button>
                    </div>
                  </article>
                ))
              ) : (
                <div className="emptyState">
                  <strong>Library is empty</strong>
                  <span>Import a paper first, then it will appear here for revisit.</span>
                </div>
              )}
            </div>
          </section>
        ) : null}

        {activeView === 'model' ? (
          <section className="card pageCard">
            <span className="eyebrow">Model settings</span>
            <h2>Connection baseline</h2>
            <p className="muted">Reader runtime now restores the most recently selected saved model. You can give presets a name, save multiple endpoints, and switch between them later.</p>
            <div className="formGrid">
              <select value={selectedModelId} onChange={(event) => void handleSelectSavedModel(event.target.value)}>
                <option value="">Create a new model preset</option>
                {savedModels.map((item) => (
                  <option key={item.id} value={item.id}>
                    {item.displayName}{item.isRecent ? ' (recent)' : ''}{item.isDefault ? ' (default)' : ''}
                  </option>
                ))}
              </select>
            </div>
            <div className="formGrid">
              <input value={modelDraft.displayName} onChange={(event) => setModelDraft({ ...modelDraft, displayName: event.target.value })} placeholder="preset name" />
              <input value={modelDraft.provider} onChange={(event) => setModelDraft({ ...modelDraft, provider: event.target.value })} placeholder="provider" />
              <input value={modelDraft.baseUrl} onChange={(event) => setModelDraft({ ...modelDraft, baseUrl: event.target.value })} placeholder="base url" />
              <input value={modelDraft.modelName} onChange={(event) => setModelDraft({ ...modelDraft, modelName: event.target.value })} placeholder="model name" />
              <select value={modelDraft.apiType ?? 'chat_completions'} onChange={(event) => setModelDraft({ ...modelDraft, apiType: event.target.value || null })}>
                <option value="chat_completions">chat/completions</option>
                <option value="responses">responses</option>
              </select>
              <input value={modelDraft.agentType ?? ''} onChange={(event) => setModelDraft({ ...modelDraft, agentType: event.target.value || null })} placeholder="agent type override (optional)" />
              <div className="row">
                <input type={showApiKey ? 'text' : 'password'} value={modelDraft.apiKey} onChange={(event) => setModelDraft({ ...modelDraft, apiKey: event.target.value })} placeholder="api key" />
                <button className="secondaryButton" type="button" onClick={() => setShowApiKey((current) => !current)}>
                  {showApiKey ? 'Hide key' : 'Show key'}
                </button>
              </div>
              <select value={modelDraft.isDefault ? 'yes' : 'no'} onChange={(event) => setModelDraft({ ...modelDraft, isDefault: event.target.value === 'yes' })}>
                <option value="yes">Use as default runtime config</option>
                <option value="no">Store as agent-specific config only</option>
              </select>
            </div>
            <div className="row rowWrap">
              <button onClick={() => void handleSaveModel()}>{selectedModelId ? 'Update preset' : 'Save preset'}</button>
              <button className="secondaryButton" onClick={() => void handleTestModel()}>Test endpoint</button>
              {selectedModelId ? <button className="secondaryButton" onClick={() => void handleDeleteSelectedModel()}>Delete preset</button> : null}
            </div>
            {modelStatus ? (
              <div className="listItem topGap">
                <strong>{modelStatus.modelIdentity}</strong>
                <span>Connected: {String(modelStatus.connected)}</span>
                <span>API type: {modelStatus.apiType}</span>
                <span>Endpoint: {modelStatus.endpoint}</span>
                <span>Latency: {modelStatus.latencyMs} ms</span>
                <span>{modelStatus.statusText}</span>
              </div>
            ) : null}
          </section>
        ) : null}
      </main>
    </div>
  );
}

async function waitForAgentRun(runId: string, maxAttempts = 10, intervalMs = 1500) {
  for (let attempt = 0; attempt < maxAttempts; attempt += 1) {
    const detail = await getAgentRun({ runId });
    if (detail.status !== 'running') {
      return detail;
    }

    await delay(intervalMs);
  }

  return getAgentRun({ runId });
}

function delay(ms: number) {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

type ParseStatusEvent = {
  eventId: string;
  paperId: string;
  runId: string | null;
  parseStatus: string;
  stage: string;
  progress: number;
  updatedAt: string;
};

function formatError(error: unknown) {
  if (isErrorPayload(error)) {
    return error.message;
  }

  if (error instanceof Error) {
    return error.message;
  }

  return 'Unknown error';
}

function isErrorPayload(error: unknown): error is { code?: string; message: string } {
  return typeof error === 'object' && error !== null && 'message' in error;
}

function isNotFoundError(error: unknown) {
  return isErrorPayload(error) && error.code === 'NOT_FOUND';
}

function getFileNameFromPath(path: string) {
  const normalized = path.trim().replace(/\\/g, '/');
  const segments = normalized.split('/').filter(Boolean);
  return segments.length > 0 ? segments[segments.length - 1] : path.trim();
}

function formatRunErrorMessage(message: string | null) {
  if (!message) {
    return 'The run failed before producing output. Check Model Settings and backend logs for the exact cause.';
  }

  return message;
}

function buildWorkflowSteps(currentStep: string | null) {
  const ordered = [
    { id: 'paper_ready', label: 'Paper ready' },
    { id: 'quick_read_completed', label: 'Quick read' },
    { id: 'careful_read_completed', label: 'Careful read' },
    { id: 'deep_read_completed', label: 'Deep read' },
    { id: 'summary_completed', label: 'Summary' },
  ];

  const currentIndex = ordered.findIndex((item) => item.id === currentStep);

  return ordered.map((item, index) => ({
    ...item,
    state: currentStep === 'workflow_blocked' ? 'blocked' : index < currentIndex ? 'done' : index === currentIndex ? 'active' : 'pending',
    caption: currentStep === 'workflow_blocked' ? 'Blocked' : index < currentIndex ? 'Completed' : index === currentIndex ? 'Current stage' : 'Waiting',
  }));
}

function renderRunSnapshot(outputSnapshot: string | null) {
  if (!outputSnapshot) {
    return 'No output snapshot available.';
  }

  try {
    const parsed = JSON.parse(outputSnapshot) as {
      summary?: string;
      evidence?: EvidenceItem[];
      shortSummary?: string;
      longSummary?: string;
      keyTakeaways?: string[];
    };

    return (
      <div className="detailGroup">
        <p>{parsed.summary ?? parsed.shortSummary ?? parsed.longSummary ?? 'No summary available.'}</p>
        {parsed.keyTakeaways && parsed.keyTakeaways.length > 0 ? (
          <ul className="detailList">
            {parsed.keyTakeaways.map((item) => (
              <li key={item}>{item}</li>
            ))}
          </ul>
        ) : null}
        {parsed.evidence && parsed.evidence.length > 0 ? (
          <div className="evidenceList">
            {parsed.evidence.map((item, index) => (
              <article className="evidenceCard" key={`${item.locator}-${index}`}>
                <strong>{item.section}</strong>
                <p>{item.quote}</p>
                <span>{formatEvidenceMeta(item)}</span>
              </article>
            ))}
          </div>
        ) : null}
        <details>
          <summary>Raw JSON</summary>
          <pre>{JSON.stringify(parsed, null, 2)}</pre>
        </details>
      </div>
    );
  } catch {
    return outputSnapshot;
  }
}

function formatEvidenceMeta(item: EvidenceItem) {
  const page = item.page ? `Page ${item.page}` : 'Page n/a';
  return `${page} · ${item.locator}`;
}
