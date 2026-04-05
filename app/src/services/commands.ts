import { invoke } from '@tauri-apps/api/core';
import {
  confirmPaperMetadataResponseSchema,
  agentRunDetailSchema,
  githubUploadTestResponseSchema,
  importPaperFromFileResponseSchema,
  libraryItemMutationResponseSchema,
  libraryListResponseSchema,
  modelConnectionResponseSchema,
  modelConfigDetailResponseSchema,
  modelConfigListResponseSchema,
  modelConfigResponseSchema,
  paperParseStatusResponseSchema,
  paperSearchResponseSchema,
  paperVisualArtifactsResponseSchema,
  profileResponseSchema,
  readerSnapshotSchema,
  runAgentResponseSchema,
} from '../schemas/contracts';
import type {
  AgentRunDetail,
  ConfirmPaperMetadataRequest,
  ConfirmPaperMetadataResponse,
  GetAgentRunRequest,
  GetPaperParseStatusRequest,
  GetPaperVisualArtifactsRequest,
  GetReaderSnapshotRequest,
  GitHubUploadTestResponse,
  ImportPaperFromFileRequest,
  ImportPaperFromFileResponse,
  ImportPaperFromLinkRequest,
  LibraryItemMutationResponse,
  LibraryListRequest,
  LibraryListResponse,
  DeleteModelConfigRequest,
  ModelConfigDetailResponse,
  ModelConfigListResponse,
  ModelConfigRequest,
  ModelConfigResponse,
  PaperParseStatusResponse,
  PaperVisualArtifactsResponse,
  ReaderSnapshot,
  RunAgentRequest,
  RunAgentResponse,
  SaveToLibraryRequest,
  SearchPapersRequest,
  SearchPapersResponse,
  SelectModelConfigRequest,
  TestModelConnectionRequest,
  TestModelConnectionResponse,
  UpdateModelConfigRequest,
  UpdateLibraryItemRequest,
  UserProfile,
} from '../types/contracts';

export async function searchPapers(payload: SearchPapersRequest): Promise<SearchPapersResponse> {
  const response = await invoke('search_papers', { request: payload });
  return paperSearchResponseSchema.parse(response);
}

export async function getProfile(): Promise<UserProfile> {
  const response = await invoke('get_profile');
  return profileResponseSchema.parse(response);
}

export async function upsertProfile(payload: UserProfile): Promise<UserProfile> {
  const request = {
    role: payload.role,
    researchField: payload.researchField,
    focusTopic: payload.focusTopic,
    readingGoal: payload.readingGoal,
    outputLanguage: payload.outputLanguage,
    experienceLevel: payload.experienceLevel,
    githubRepoOwner: payload.githubRepoOwner,
    githubRepoName: payload.githubRepoName,
    githubRepoBranch: payload.githubRepoBranch,
    githubRepoPathPrefix: payload.githubRepoPathPrefix,
    githubCdnBaseUrl: payload.githubCdnBaseUrl,
    githubToken: payload.githubToken,
  };
  const response = await invoke('upsert_profile', { request });
  return profileResponseSchema.parse(response);
}

export async function testGitHubUpload(): Promise<GitHubUploadTestResponse> {
  const response = await invoke('test_github_upload');
  return githubUploadTestResponseSchema.parse(response);
}

export async function saveModelConfig(payload: ModelConfigRequest): Promise<ModelConfigResponse> {
  const response = await invoke('save_model_config', { request: payload });
  return modelConfigResponseSchema.parse(response);
}

export async function updateModelConfig(payload: UpdateModelConfigRequest): Promise<ModelConfigResponse> {
  const response = await invoke('update_model_config', { request: payload });
  return modelConfigResponseSchema.parse(response);
}

export async function listModelConfigs(): Promise<ModelConfigListResponse> {
  const response = await invoke('list_model_configs');
  return modelConfigListResponseSchema.parse(response);
}

export async function getModelConfigDetail(payload: SelectModelConfigRequest): Promise<ModelConfigDetailResponse> {
  const response = await invoke('get_model_config_detail', { request: payload });
  return modelConfigDetailResponseSchema.parse(response);
}

export async function getRecentModelConfig(): Promise<ModelConfigResponse> {
  const response = await invoke('get_recent_model_config');
  return modelConfigResponseSchema.parse(response);
}

export async function selectModelConfig(payload: SelectModelConfigRequest): Promise<ModelConfigResponse> {
  const response = await invoke('select_model_config', { request: payload });
  return modelConfigResponseSchema.parse(response);
}

export async function deleteModelConfig(payload: DeleteModelConfigRequest): Promise<void> {
  await invoke('delete_model_config', { request: payload });
}

export async function testModelConnection(
  payload: TestModelConnectionRequest,
): Promise<TestModelConnectionResponse> {
  const response = await invoke('test_model_connection', { request: payload });
  return modelConnectionResponseSchema.parse(response);
}

export async function importPaperFromFile(
  payload: ImportPaperFromFileRequest,
): Promise<ImportPaperFromFileResponse> {
  const response = await invoke('import_paper_from_file', { request: payload });
  return importPaperFromFileResponseSchema.parse(response);
}

export async function pickPdfFile(): Promise<string | null> {
  const response = await invoke<string | null>('pick_pdf_file');
  return response;
}

export async function importPaperFromLink(
  payload: ImportPaperFromLinkRequest,
): Promise<ImportPaperFromFileResponse> {
  const response = await invoke('import_paper_from_link', { request: payload });
  return importPaperFromFileResponseSchema.parse(response);
}

export async function confirmPaperMetadata(
  payload: ConfirmPaperMetadataRequest,
): Promise<ConfirmPaperMetadataResponse> {
  const response = await invoke('confirm_paper_metadata', { request: payload });
  return confirmPaperMetadataResponseSchema.parse(response);
}

export async function reparsePaper(
  payload: GetPaperParseStatusRequest,
): Promise<void> {
  await invoke('reparse_paper', { request: payload });
}

export async function getPaperParseStatus(
  payload: GetPaperParseStatusRequest,
): Promise<PaperParseStatusResponse> {
  const response = await invoke('get_paper_parse_status', { request: payload });
  return paperParseStatusResponseSchema.parse(response);
}

export async function getReaderSnapshot(
  payload: GetReaderSnapshotRequest,
): Promise<ReaderSnapshot> {
  const response = await invoke('get_reader_snapshot', { request: payload });
  return readerSnapshotSchema.parse(response);
}

export async function getPaperVisualArtifacts(
  payload: GetPaperVisualArtifactsRequest,
): Promise<PaperVisualArtifactsResponse> {
  const response = await invoke('get_paper_visual_artifacts', { request: payload });
  return paperVisualArtifactsResponseSchema.parse(response);
}

export async function runAgent(payload: RunAgentRequest): Promise<RunAgentResponse> {
  const response = await invoke('run_agent', { request: payload });
  return runAgentResponseSchema.parse(response);
}

export async function getAgentRun(payload: GetAgentRunRequest): Promise<AgentRunDetail> {
  const response = await invoke('get_agent_run', { request: payload });
  return agentRunDetailSchema.parse(response);
}

export async function listLibraryItems(payload: LibraryListRequest): Promise<LibraryListResponse> {
  const response = await invoke('list_library_items', { request: payload });
  return libraryListResponseSchema.parse(response);
}

export async function saveToLibrary(
  payload: SaveToLibraryRequest,
): Promise<LibraryItemMutationResponse> {
  const response = await invoke('save_to_library', { request: payload });
  return libraryItemMutationResponseSchema.parse(response);
}

export async function updateLibraryItem(
  payload: UpdateLibraryItemRequest,
): Promise<LibraryItemMutationResponse> {
  const response = await invoke('update_library_item', { request: payload });
  return libraryItemMutationResponseSchema.parse(response);
}
