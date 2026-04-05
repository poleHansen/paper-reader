import { useEffect, useState } from 'react';
import { convertFileSrc } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import {
  confirmPaperMetadata,
  deleteModelConfig,
  getAgentRun,
  getModelConfigDetail,
  getRecentModelConfig,
  getPaperParseStatus,
  getPaperVisualArtifacts,
  getProfile,
  getReaderSnapshot,
  importPaperFromFile,
  importPaperFromLink,
  listModelConfigs,
  listLibraryItems,
  pickPdfFile,
  reparsePaper,
  runAgent,
  saveModelConfig,
  selectModelConfig,
  testGitHubUpload,
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
  PaperVisualArtifactsResponse,
  ParsedFigure,
  ParsedTable,
  ReaderSnapshot,
  UserProfile,
  VisualDiagnostic,
} from '../types/contracts';

type View = 'dashboard' | 'onboarding' | 'search' | 'upload' | 'reader' | 'visuals' | 'library' | 'model';

const navigationItems: Array<{ view: View; label: string }> = [
  { view: 'dashboard', label: 'Dashboard' },
  { view: 'onboarding', label: 'User Profile' },
  { view: 'search', label: 'Search' },
  { view: 'upload', label: 'Upload' },
  { view: 'reader', label: 'Reader' },
  { view: 'visuals', label: 'Visual Artifacts' },
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
  githubRepoOwner: null,
  githubRepoName: null,
  githubRepoBranch: null,
  githubRepoPathPrefix: null,
  githubCdnBaseUrl: null,
  githubToken: null,
  hasGithubToken: false,
};

export function App() {
  const [activeView, setActiveView] = useState<View>('dashboard');
  const [query, setQuery] = useState('agent memory');
  const [searchResults, setSearchResults] = useState<PaperSearchResult[]>([]);
  const [libraryItems, setLibraryItems] = useState<LibraryItem[]>([]);
  const [profile, setProfile] = useState<UserProfile>(defaultProfile);
  const [statusText, setStatusText] = useState('Ready');
  const [isTestingGitHubUpload, setIsTestingGitHubUpload] = useState(false);
  const [modelStatus, setModelStatus] = useState<ModelConnectionResult | null>(null);
  const [savedModels, setSavedModels] = useState<ModelConfigResponse[]>([]);
  const [selectedModelId, setSelectedModelId] = useState('');
  const [showApiKey, setShowApiKey] = useState(false);
  const [showGithubToken, setShowGithubToken] = useState(false);
  const [modelDraft, setModelDraft] = useState<ModelConfigRequest>({
    displayName: 'Default OpenAI-compatible',
    provider: 'openai_compatible',
    baseUrl: 'https://api.openai.com/v1',
    modelName: 'gpt-4.1-mini',
    apiKey: '',
    apiType: 'chat_completions',
    imageInputFormat: null,
    agentType: null,
    isDefault: true,
  });
  const [filePath, setFilePath] = useState('');
  const [paperUrl, setPaperUrl] = useState('');
  const [selectedPaperId, setSelectedPaperId] = useState<string | null>(null);
  const [readerSnapshot, setReaderSnapshot] = useState<ReaderSnapshot | null>(null);
  const [visualArtifacts, setVisualArtifacts] = useState<PaperVisualArtifactsResponse | null>(null);
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
  const effectiveGithubBranch = profile.githubRepoBranch?.trim() || 'main';
  const hasGithubTokenValue = Boolean(profile.githubToken?.trim() || profile.hasGithubToken);
  const githubHostingReady = Boolean(
    profile.githubRepoOwner?.trim()
    && profile.githubRepoName?.trim()
    && hasGithubTokenValue,
  );
  const modelNeedsGithubHosting = modelDraft.imageInputFormat === 'url_required'
    || modelStatus?.imageInputWorkingFormat === 'url_required';

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
      setProfile((current) => ({
        ...saved,
        githubToken: saved.githubToken ?? current.githubToken,
        hasGithubToken: saved.hasGithubToken ?? Boolean(saved.githubToken ?? current.githubToken?.trim()),
      }));
      setHasProfile(true);
      setActiveView('search');
      setStatusText('Profile saved');
    } catch (error) {
      setStatusText(formatError(error));
    }
  }

  async function handleTestGitHubUpload() {
    if (!profile.githubRepoOwner?.trim() || !profile.githubRepoName?.trim()) {
      setStatusText('GitHub upload test requires repo owner and repo name in User Profile');
      return;
    }
    if (!hasGithubTokenValue) {
      setStatusText('GitHub upload test requires a saved GitHub token. Enter a token and click Save profile first.');
      return;
    }

    try {
      setIsTestingGitHubUpload(true);
      setStatusText('Testing GitHub upload by sending a small PNG to the configured repository...');
      const result = await testGitHubUpload();
      setStatusText(`GitHub upload test succeeded: ${result.repositoryPath} -> ${result.publicUrl}`);
    } catch (error) {
      setStatusText(formatError(error));
    } finally {
      setIsTestingGitHubUpload(false);
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
      imageInputFormat: model.imageInputFormat,
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
        imageInputFormat: null,
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
        imageInputFormat: modelDraft.imageInputFormat,
      });
      setModelDraft((current) => ({
        ...current,
        apiType: result.apiType,
        imageInputFormat: result.imageInputWorkingFormat,
      }));
      setModelStatus(result);
      setStatusText(
        result.connected
          ? `Model request succeeded. ${result.imageInputMessage}`
          : result.statusText,
      );
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
      if (!result.metadataNeedsConfirmation) {
        await loadReaderSnapshot(result.paperId);
      } else {
        setReaderSnapshot(null);
        setVisualArtifacts(null);
      }
      await refreshLibrary();
      setActiveView(result.metadataNeedsConfirmation ? 'upload' : 'reader');
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
      if (!result.metadataNeedsConfirmation) {
        await loadReaderSnapshot(result.paperId);
      } else {
        setReaderSnapshot(null);
        setVisualArtifacts(null);
      }
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
      setActiveRun((current) => {
        if (current && snapshot.latestAgentRuns.some((run) => run.id === current.id)) {
          return current;
        }

        return null;
      });

      if (snapshot.latestAgentRuns.length > 0) {
        const latestRunId = snapshot.latestAgentRuns[0].id;
        void getAgentRun({ runId: latestRunId })
          .then(setActiveRun)
          .catch((error) => setStatusText(formatError(error)));
      }

      if (snapshot.parsedContent) {
        const artifacts = await getPaperVisualArtifacts({ paperId });
        setVisualArtifacts(artifacts);
      } else {
        setVisualArtifacts(null);
      }
    } catch (error) {
      setStatusText(formatError(error));
      setVisualArtifacts(null);
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

  async function handleReparsePaper() {
    if (!selectedPaperId) {
      setStatusText('Open a paper before starting reparse');
      return;
    }

    try {
      await reparsePaper({ paperId: selectedPaperId });
      setStatusText('Reparse started. This pass will regenerate visual assets and upload GitHub-hosted images when visual parsing runs.');
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
              <input value={profile.outputLanguage} onChange={(event) => setProfile({ ...profile, outputLanguage: event.target.value })} placeholder="output language" />
              <input value={profile.experienceLevel} onChange={(event) => setProfile({ ...profile, experienceLevel: event.target.value })} placeholder="experience level" />
              <input value={profile.githubRepoOwner ?? ''} onChange={(event) => setProfile({ ...profile, githubRepoOwner: event.target.value || null })} placeholder="github repo owner" />
              <input value={profile.githubRepoName ?? ''} onChange={(event) => setProfile({ ...profile, githubRepoName: event.target.value || null })} placeholder="github repo name" />
              <input value={profile.githubRepoBranch ?? ''} onChange={(event) => setProfile({ ...profile, githubRepoBranch: event.target.value || null })} placeholder="github branch" />
              <input value={profile.githubRepoPathPrefix ?? ''} onChange={(event) => setProfile({ ...profile, githubRepoPathPrefix: event.target.value || null })} placeholder="github path prefix (optional)" />
              <input value={profile.githubCdnBaseUrl ?? ''} onChange={(event) => setProfile({ ...profile, githubCdnBaseUrl: event.target.value || null })} placeholder="github cdn/raw base url (optional)" />
              <div className="row rowWrap">
                <input
                  type={showGithubToken ? 'text' : 'password'}
                  value={profile.githubToken ?? ''}
                  onChange={(event) => setProfile({ ...profile, githubToken: event.target.value || null })}
                  placeholder={hasGithubTokenValue ? 'github token already saved; enter to replace' : 'github token'}
                />
                <button className="secondaryButton" type="button" onClick={() => setShowGithubToken((value) => !value)}>
                  {showGithubToken ? 'Hide token' : 'Show token'}
                </button>
              </div>
            </div>
            <div className={githubHostingReady ? 'noticeCard noticeCardSuccess' : 'noticeCard'}>
              <strong>GitHub image hosting</strong>
              <span>
                {githubHostingReady
                  ? `Ready: ${profile.githubRepoOwner}/${profile.githubRepoName}@${effectiveGithubBranch}`
                  : 'Not fully configured yet. Public image URL fallback will stay unavailable until owner, repo, and token are all set. Branch defaults to main when left blank.'}
              </span>
              {!profile.githubRepoBranch?.trim() && githubHostingReady ? <span>Branch default: main</span> : null}
              {profile.githubRepoPathPrefix ? <span>Path prefix: {profile.githubRepoPathPrefix}</span> : null}
              {profile.githubCdnBaseUrl ? <span>CDN base: {profile.githubCdnBaseUrl}</span> : null}
            </div>
            <div className="row rowEnd rowWrap">
              <button className="secondaryButton" onClick={() => void handleTestGitHubUpload()} disabled={isTestingGitHubUpload}>
                {isTestingGitHubUpload ? 'Testing GitHub upload...' : 'Test GitHub upload'}
              </button>
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
          <section className="card pageCard pageCardReaderModern">
            <div className="sectionHeader">
              <div>
                <span className="eyebrow">Reader</span>
                <h2>Paper workspace</h2>
                <p className="muted">Read the original paper on the left and review the current agent output on the right.</p>
              </div>
              <div className="row rowWrap readerHeaderActions">
                {readerSnapshot?.allowedActions.includes('run_quick_read') ? <button onClick={() => void handleRunAgent('quick_read')}>Quick read</button> : null}
                {readerSnapshot?.allowedActions.includes('run_careful_read') ? <button onClick={() => void handleRunAgent('careful_read')}>Careful read</button> : null}
                {readerSnapshot?.allowedActions.includes('run_deep_read') ? <button onClick={() => void handleRunAgent('deep_read')}>Deep read</button> : null}
                {readerSnapshot?.allowedActions.includes('run_summary') ? <button onClick={() => void handleRunAgent('summary')}>Summary</button> : null}
                <button
                  className="secondaryButton"
                  onClick={() => setActiveView('visuals')}
                  disabled={!selectedPaperId}
                >
                  Open visual artifacts
                </button>
                <button className="secondaryButton" onClick={() => void handleReparsePaper()} disabled={!selectedPaperId}>Reparse paper</button>
                <button onClick={() => void handleRefreshParseStatus()} disabled={!selectedPaperId}>Refresh status</button>
              </div>
            </div>
            {readerSnapshot ? (
              <>
                <div className="workflowStrip">
                  <article className="workflowStripCard">
                    <span className="detailLabel">Parse</span>
                    <strong>{formatParseStatus(readerSnapshot.parseStatus)}</strong>
                  </article>
                  <article className="workflowStripCard">
                    <span className="detailLabel">Progress</span>
                    <strong>{readerSnapshot.parseProgress}%</strong>
                  </article>
                  <article className="workflowStripCard">
                    <span className="detailLabel">Current step</span>
                    <strong>{formatWorkflowStep(readerSnapshot.workflowCurrentStep)}</strong>
                  </article>
                  <article className="workflowStripCard">
                    <span className="detailLabel">Next action</span>
                    <strong>{formatActionLabel(readerSnapshot.nextActionRequired) ?? 'None'}</strong>
                  </article>
                  <article className="workflowStripCard">
                    <span className="detailLabel">Library</span>
                    <strong>{formatLibraryStatus(readerSnapshot.libraryStatus)}</strong>
                  </article>
                </div>

                <div className="readerSplitLayout">
                  <section className="readerDocumentPane">
                    <div className="readerPaneHeader">
                      <div>
                        <span className="eyebrow">Paper</span>
                        <strong>{readerSnapshot.title}</strong>
                        <p className="muted">
                          {readerSnapshot.authors.length > 0 ? readerSnapshot.authors.join(', ') : 'Authors unavailable'}
                          {' · '}
                          {readerSnapshot.venue ?? 'Unknown venue'}
                          {' · '}
                          {readerSnapshot.year ?? 'unknown year'}
                        </p>
                      </div>
                    </div>
                    <div className="readerDocumentViewport">
                      {renderPaperDocument(readerSnapshot)}
                    </div>
                  </section>

                  <section className="readerAgentPane">
                    <div className="readerPaneHeader">
                      <div>
                        <span className="eyebrow">Analysis</span>
                        <strong>Current agent output</strong>
                        <p className="muted">This panel focuses on the latest reading result instead of workflow state.</p>
                      </div>
                    </div>

                    <div className="readerAgentBody">
                      {readerSnapshot.activeRun ? (
                        <section className="readerPanel">
                          <span className="detailLabel">Running now</span>
                          <strong>{readerSnapshot.activeRun.agentType}</strong>
                          <span>Batch {readerSnapshot.activeRun.currentBatchIndex} / {readerSnapshot.activeRun.currentBatchCount}</span>
                          <span>Sections: {readerSnapshot.activeRun.contextPlan.selectedSectionIds.join(', ') || 'none'}</span>
                          <span>Mode: {readerSnapshot.activeRun.contextPlan.runtimeMode} / {readerSnapshot.activeRun.contextPlan.sectionStrategy}</span>
                        </section>
                      ) : null}

                      {readerSnapshot.latestAgentRuns.length > 0 ? (
                        <section className="readerRunSwitcher">
                          <span className="detailLabel">Recent runs</span>
                          <div className="runChipList">
                            {readerSnapshot.latestAgentRuns.map((run) => (
                              <button
                                key={run.id}
                                className={`runChip ${activeRun?.id === run.id ? 'runChipActive' : ''}`}
                                onClick={() => {
                                  void getAgentRun({ runId: run.id })
                                    .then(setActiveRun)
                                    .catch((error) => setStatusText(formatError(error)));
                                }}
                              >
                                {run.agentType}
                              </button>
                            ))}
                          </div>
                        </section>
                      ) : null}

                      {activeRun ? (
                        <section className="readerPanel readerPanelScrollable">
                          <span className="detailLabel">Selected result</span>
                          <div className="runCard">
                            <strong>{activeRun.agentType}</strong>
                            <span>Status: {activeRun.status}</span>
                            <span>Finished: {activeRun.finishedAt ?? 'n/a'}</span>
                            {activeRun.contextPlan ? (
                              <div className="detailGroup">
                                <span className="detailLabel">Context plan</span>
                                <span>Mode: {activeRun.contextPlan.runtimeMode}</span>
                                <span>Strategy: {activeRun.contextPlan.sectionStrategy}</span>
                                <p>{activeRun.contextPlan.selectionReason}</p>
                                <span>Sections: {activeRun.contextPlan.selectedSectionIds.join(', ') || 'none'}</span>
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
                        </section>
                      ) : (
                        <div className="emptyState compactEmpty">
                          <strong>No selected result yet</strong>
                          <span>Run an agent from the workflow bar or choose one of the recent run buttons.</span>
                        </div>
                      )}
                    </div>
                  </section>
                </div>
              </>
            ) : (
              <div className="emptyState">
                <strong>Reader is waiting for a paper</strong>
                <span>Import a PDF or open an item from the library to load the workspace.</span>
              </div>
            )}
          </section>
        ) : null}

        {activeView === 'visuals' ? (
          <section className="card pageCard pageCardReaderModern">
            <div className="sectionHeader">
              <div>
                <span className="eyebrow">Visual artifacts</span>
                <h2>Figures, tables, and evidence</h2>
                <p className="muted">Visual parsing results are separated from the reading workspace so the Reader can stay focused.</p>
              </div>
              <div className="row rowWrap">
                <button className="secondaryButton" onClick={() => setActiveView('reader')} disabled={!selectedPaperId}>Back to reader</button>
                <button onClick={() => void handleRefreshParseStatus()} disabled={!selectedPaperId}>Refresh status</button>
              </div>
            </div>
            {readerSnapshot ? (
              <div className="visualsPageLayout">
                <section className="readerPanel">
                  <span className="eyebrow">Overview</span>
                  <strong>
                    Figures {readerSnapshot.parsedContent?.figureCount ?? 0} · Tables {readerSnapshot.parsedContent?.tableCount ?? 0}
                  </strong>
                  <span>Visual mode: {readerSnapshot.parsedContent?.visualMode ?? 'disabled'}</span>
                  <span>
                    Visual parsing: {readerSnapshot.parsedContent?.visualMode === 'multimodal'
                      ? 'multimodal interpreted'
                      : readerSnapshot.parsedContent?.visualEnabled
                        ? 'fallback extraction'
                        : 'disabled'}
                  </span>
                  <span>Summaries: {readerSnapshot.parsedContent?.visualSummaryCount ?? 0}</span>
                  {readerSnapshot.parsedContent?.sampleCaption ? (
                    <div className="visualPreviewCard">
                      <span className="detailLabel">Sample caption</span>
                      <strong>{readerSnapshot.parsedContent.sampleCaption}</strong>
                      <span>{readerSnapshot.parsedContent.sampleSummary ?? 'No visual summary generated yet.'}</span>
                    </div>
                  ) : null}
                  <span>Warnings: {readerSnapshot.parsedContent?.visualWarnings.join(' | ') || 'none'}</span>
                  {renderGitHubUploadDiagnostics(readerSnapshot.parsedContent?.githubUploadDiagnostics ?? [])}
                  {renderVisualDiagnostics(readerSnapshot.parsedContent?.visualDiagnostics ?? [])}
                </section>

                <section className="readerPanel readerPanelScrollable">
                  {visualArtifacts && (visualArtifacts.figures.length > 0 || visualArtifacts.tables.length > 0 || visualArtifacts.visualEvidence.length > 0) ? (
                    <div className="detailGroup">
                      {renderGitHubUploadDiagnostics(visualArtifacts.githubUploadDiagnostics)}
                      {renderVisualDiagnostics(visualArtifacts.visualDiagnostics)}
                      {visualArtifacts.figures.length > 0 ? (
                        <div className="detailGroup">
                          <span className="detailLabel">Figures</span>
                          <div className="artifactList">
                            {visualArtifacts.figures.map((figure) => (
                              <article className="artifactCard" key={figure.id}>
                                {renderVisualPreview(figure)}
                                <div className="artifactContent">
                                  <div className="artifactHeader">
                                    <strong>{figure.label || figure.id}</strong>
                                    <span>{formatArtifactMeta(figure.page, figure.locator, figure.confidence)}</span>
                                  </div>
                                  {figure.title ? <span className="artifactTitle">{figure.title}</span> : null}
                                  <p>{figure.caption}</p>
                                  <div className="detailGroup">
                                    <span className="detailLabel">Model reading</span>
                                    <span>{figureSummaryMode(visualArtifacts, figure.id)}</span>
                                    <span>{displayVisualSummary(visualArtifacts, figure.id, figure.summary)}</span>
                                  </div>
                                  {figure.ocrText.length > 0 ? (
                                    <details>
                                      <summary>OCR text</summary>
                                      <pre>{figure.ocrText.join('\n')}</pre>
                                    </details>
                                  ) : null}
                                </div>
                              </article>
                            ))}
                          </div>
                        </div>
                      ) : null}
                      {visualArtifacts.tables.length > 0 ? (
                        <div className="detailGroup">
                          <span className="detailLabel">Tables</span>
                          <div className="artifactList">
                            {visualArtifacts.tables.map((table) => (
                              <article className="artifactCard" key={table.id}>
                                {renderVisualPreview(table)}
                                <div className="artifactContent">
                                  <div className="artifactHeader">
                                    <strong>{table.label || table.id}</strong>
                                    <span>{formatArtifactMeta(table.page, table.locator, table.confidence)}</span>
                                  </div>
                                  {table.title ? <span className="artifactTitle">{table.title}</span> : null}
                                  <p>{table.caption}</p>
                                  <div className="detailGroup">
                                    <span className="detailLabel">Model reading</span>
                                    <span>{figureSummaryMode(visualArtifacts, table.id)}</span>
                                    <span>{displayVisualSummary(visualArtifacts, table.id, table.summary)}</span>
                                  </div>
                                  {table.markdownTable ? (
                                    <details>
                                      <summary>Extracted table</summary>
                                      <pre>{table.markdownTable}</pre>
                                    </details>
                                  ) : null}
                                  {table.ocrText.length > 0 ? (
                                    <details>
                                      <summary>OCR text</summary>
                                      <pre>{table.ocrText.join('\n')}</pre>
                                    </details>
                                  ) : null}
                                </div>
                              </article>
                            ))}
                          </div>
                        </div>
                      ) : null}
                      {visualArtifacts.visualEvidence.length > 0 ? (
                        <div className="detailGroup">
                          <span className="detailLabel">Visual evidence</span>
                          <div className="evidenceList">
                            {visualArtifacts.visualEvidence.map((item) => (
                              <article className="evidenceCard" key={item.id}>
                                <strong>{item.claim}</strong>
                                <span>{formatVisualEvidenceMeta(item.supportLevel, item.sourceObjectType, item.sourceObjectId, item.page, item.confidence)}</span>
                                <p>{item.evidenceText}</p>
                                <span>{item.locator}</span>
                              </article>
                            ))}
                          </div>
                        </div>
                      ) : null}
                    </div>
                  ) : (
                    <p>No extracted figures or tables yet. The parser will use multimodal interpretation when the selected model supports images, then fall back to caption-based extraction.</p>
                  )}
                </section>
              </div>
            ) : (
              <div className="emptyState">
                <strong>Visual artifacts are waiting for a paper</strong>
                <span>Open a paper in the Reader first, then come back here for figure and table details.</span>
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
            {modelNeedsGithubHosting ? (
              <div className={githubHostingReady ? 'noticeCard noticeCardSuccess' : 'noticeCard noticeCardWarning'}>
                <strong>Public image URL required</strong>
                <span>
                  This model route rejects inline image payloads. Figure and table recognition will upload extracted images to your configured GitHub repository and then send the public URL.
                </span>
                <span>
                  {githubHostingReady
                    ? 'GitHub hosting is configured, so visual parsing can use the fallback path.'
                    : 'GitHub hosting is not ready yet. Configure owner, repo, and token in User Profile before running visual parsing. Branch defaults to main when left blank.'}
                </span>
              </div>
            ) : null}
            {modelStatus ? (
              <div className="listItem topGap">
                <strong>{modelStatus.modelIdentity}</strong>
                <span>Connected: {String(modelStatus.connected)}</span>
                <span>API type: {modelStatus.apiType}</span>
                <span>Endpoint: {modelStatus.endpoint}</span>
                <span>Latency: {modelStatus.latencyMs} ms</span>
                <span>{modelStatus.statusText}</span>
                <span>Image input: {modelStatus.imageInputSupported ? 'supported' : 'not verified'}</span>
                <span>{modelStatus.imageInputMessage}</span>
                <span>
                  Working format: {modelStatus.imageInputWorkingFormat ?? 'none'}
                </span>
                <span>
                  Attempted formats: {modelStatus.imageProbeAttemptedFormats.join(', ')}
                </span>
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

function resolveAssetUrl(path: string | null) {
  if (!path) {
    return null;
  }

  const normalized = path.trim().replace(/\\/g, '/');
  if (!normalized) {
    return null;
  }

  if (
    normalized.startsWith('asset:')
    || normalized.startsWith('http://')
    || normalized.startsWith('https://')
    || normalized.startsWith('data:')
    || normalized.startsWith('blob:')
  ) {
    return normalized;
  }

  return convertFileSrc(normalized);
}

function renderPaperDocument(readerSnapshot: ReaderSnapshot) {
  const documentUrl = resolveAssetUrl(readerSnapshot.storagePath);

  if (documentUrl) {
    return (
      <iframe
        className="readerDocumentFrame"
        src={documentUrl}
        title={readerSnapshot.title}
      />
    );
  }

  return (
    <div className="emptyState compactEmpty">
      <strong>Paper preview unavailable</strong>
      <span>The current snapshot does not expose a readable PDF path yet.</span>
      <span>File: {readerSnapshot.fileName ?? 'No local file recorded'}</span>
      <span>Source: {readerSnapshot.source}</span>
    </div>
  );
}

function renderVisualPreview(item: ParsedFigure | ParsedTable) {
  const previewUrl = resolveAssetUrl(item.thumbnailPath ?? item.imagePath);
  if (!previewUrl) {
    return (
      <div className="artifactPreview artifactPreviewPlaceholder">
        <span>No preview</span>
      </div>
    );
  }

  return <img className="artifactPreview" src={previewUrl} alt={item.label || item.id} />;
}

function formatArtifactMeta(page: number | null, locator: string, confidence: number | null) {
  const parts = [page ? `p.${page}` : null, locator || null, confidence != null ? `confidence ${Math.round(confidence * 100)}%` : null].filter(Boolean);
  return parts.join(' · ') || 'No locator';
}

function formatVisualEvidenceMeta(
  supportLevel: string,
  sourceObjectType: string,
  sourceObjectId: string,
  page: number | null,
  confidence: number | null,
) {
  const parts = [supportLevel, `${sourceObjectType}:${sourceObjectId}`, page ? `p.${page}` : null, confidence != null ? `confidence ${Math.round(confidence * 100)}%` : null].filter(Boolean);
  return parts.join(' · ');
}

function figureSummaryMode(visualArtifacts: PaperVisualArtifactsResponse, sourceObjectId: string) {
  const evidence = visualArtifacts.visualEvidence.find((item) => item.sourceObjectId === sourceObjectId);
  if (!evidence) {
    return 'No visual understanding result';
  }

  if (evidence.supportLevel === 'multimodal_direct') {
    return 'Multimodal interpreted';
  }

  return `Fallback: ${evidence.supportLevel}`;
}

function displayVisualSummary(
  visualArtifacts: PaperVisualArtifactsResponse,
  sourceObjectId: string,
  summary: string | null,
) {
  const evidence = visualArtifacts.visualEvidence.find((item) => item.sourceObjectId === sourceObjectId);
  if (!evidence || evidence.supportLevel !== 'multimodal_direct') {
    return 'Multimodal analysis did not complete for this artifact. Current result only confirms the caption/region match, not the chart content itself.';
  }

  if (!summary || !summary.trim()) {
    return 'Multimodal analysis completed, but the model returned no usable summary.';
  }

  return summary;
}

function renderVisualDiagnostics(items: VisualDiagnostic[]) {
  if (!items.length) {
    return null;
  }

  return (
    <div className="detailGroup">
      <span className="detailLabel">Visual diagnostics</span>
      <div className="evidenceList">
        {items.map((item, index) => (
          <article className="evidenceCard" key={`${item.scope}-${item.code}-${index}`}>
            <strong>{item.code}</strong>
            <span>{item.scope} · {item.retryable ? 'retryable' : 'non-retryable'}</span>
            <p>{item.message}</p>
          </article>
        ))}
      </div>
    </div>
  );
}

function renderGitHubUploadDiagnostics(items: VisualDiagnostic[]) {
  if (!items.length) {
    return null;
  }

  return (
    <div className="detailGroup">
      <span className="detailLabel">GitHub upload status</span>
      <div className="evidenceList">
        {items.map((item, index) => (
          <article className="evidenceCard" key={`${item.scope}-${item.code}-${index}`}>
            <strong>{item.code === 'github_upload_succeeded' ? 'upload succeeded' : 'upload failed'}</strong>
            <span>{item.scope} · {item.retryable ? 'retryable' : 'non-retryable'}</span>
            <p>{item.message}</p>
          </article>
        ))}
      </div>
    </div>
  );
}

function formatParseStatus(status: string | null) {
  switch (status) {
    case 'queued':
      return 'Queued';
    case 'running':
      return 'Running';
    case 'succeeded':
      return 'Succeeded';
    case 'failed':
      return 'Failed';
    default:
      return status ?? 'Unknown';
  }
}

function formatWorkflowStep(step: string | null) {
  switch (step) {
    case 'paper_ready':
      return 'Paper ready';
    case 'quick_read_running':
      return 'Quick read running';
    case 'quick_read_completed':
      return 'Quick read completed';
    case 'careful_read_running':
      return 'Careful read running';
    case 'careful_read_completed':
      return 'Careful read completed';
    case 'deep_read_running':
      return 'Deep read running';
    case 'deep_read_completed':
      return 'Deep read completed';
    case 'summary_running':
      return 'Summary running';
    case 'summary_completed':
      return 'Summary completed';
    case 'workflow_blocked':
      return 'Blocked';
    default:
      return step ?? 'Unknown';
  }
}

function formatActionLabel(action: string | null) {
  switch (action) {
    case 'refresh_status':
      return 'Refresh status';
    case 'cancel_run':
      return 'Cancel run';
    case 'run_quick_read':
      return 'Run quick read';
    case 'run_careful_read':
      return 'Run careful read';
    case 'run_deep_read':
      return 'Run deep read';
    case 'run_summary':
      return 'Run summary';
    case 'save_to_library':
      return 'Save to library';
    case 're_run_summary':
      return 'Re-run summary';
    case 'reopen_reader':
      return 'Reopen reader';
    default:
      return action;
  }
}

function formatLibraryStatus(status: string | null) {
  switch (status) {
    case 'queued':
      return 'Queued';
    case 'reading':
      return 'Reading';
    case 'completed':
      return 'Completed';
    case 'archived':
      return 'Archived';
    case null:
      return 'Not saved';
    default:
      return status;
  }
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
