import { useReducer } from "react";
import type {
  BranchList, BranchRefMutationResult, CommitResult, ConflictFile, HistoryMutationResult,
  MergeMutationResult, RemoteRefresh, RepositoryInfo, RepositoryOperation, RepositoryState,
  RepositoryStatus, StashMutationResult,
} from "../../lib/repository";

export type LoadState<T> =
  | { phase: "idle" | "loading"; data: null }
  | { phase: "ready"; data: T }
  | { phase: "error"; data: null; message: string };

export interface RepositorySessionState {
  repository: RepositoryInfo | null;
  status: LoadState<RepositoryStatus>;
  branches: LoadState<BranchList>;
  operation: RepositoryOperation;
  conflicts: ConflictFile[];
}

type SessionAction =
  | { type: "reset" }
  | { type: "restore"; session: RepositorySessionState }
  | { type: "opened"; repository: RepositoryInfo }
  | { type: "statusLoading" }
  | { type: "statusError"; message: string; clearOperation?: boolean }
  | { type: "statusReady"; status: RepositoryStatus }
  | { type: "repositoryState"; state: RepositoryState }
  | { type: "branchesLoading" }
  | { type: "branchesError"; message: string }
  | { type: "branchesReady"; branches: BranchList }
  | { type: "commit"; result: CommitResult }
  | { type: "checkout"; result: { branch: string | null; head: RepositoryInfo["head"]; branches: BranchList; status: RepositoryStatus } }
  | { type: "branchRefs"; result: BranchRefMutationResult }
  | { type: "stash"; result: StashMutationResult }
  | { type: "remote"; result: RemoteRefresh }
  | { type: "historyMutation"; result: MergeMutationResult | HistoryMutationResult }
  | { type: "watchIdentity"; branch: string | null; head: RepositoryInfo["head"] };

const initialState: RepositorySessionState = {
  repository: null,
  status: { phase: "idle", data: null },
  branches: { phase: "idle", data: null },
  operation: { kind: "none" },
  conflicts: [],
};

function repositorySessionReducer(state: RepositorySessionState, action: SessionAction): RepositorySessionState {
  switch (action.type) {
    case "reset": return initialState;
    case "restore": return action.session;
    case "opened": return { ...state, repository: action.repository };
    case "watchIdentity": return { ...state, repository: state.repository && { ...state.repository, branch: action.branch, head: action.head } };
    case "statusLoading": return { ...state, status: { phase: "loading", data: null } };
    case "statusError": return {
      ...state,
      status: { phase: "error", data: null, message: action.message },
      ...(action.clearOperation ? { operation: { kind: "none" as const }, conflicts: [] } : {}),
    };
    case "statusReady": return { ...state, status: { phase: "ready", data: action.status } };
    case "repositoryState": return {
      ...state,
      status: { phase: "ready", data: action.state.status },
      operation: action.state.operation,
      conflicts: action.state.conflicts,
    };
    case "branchesLoading": return { ...state, branches: { phase: "loading", data: null } };
    case "branchesError": return { ...state, branches: { phase: "error", data: null, message: action.message } };
    case "branchesReady": return { ...state, branches: { phase: "ready", data: action.branches } };
    case "commit": return {
      ...state,
      repository: state.repository && { ...state.repository, head: action.result.head },
      branches: state.branches.phase === "ready" ? {
        phase: "ready",
        data: { branches: state.branches.data.branches.map((branch) => branch.isCurrent ? { ...branch, targetHash: action.result.head.hash } : branch) },
      } : state.branches,
      status: { phase: "ready", data: action.result.status },
      operation: { kind: "none" },
      conflicts: [],
    };
    case "checkout": return {
      ...state,
      repository: state.repository && { ...state.repository, branch: action.result.branch, head: action.result.head },
      branches: { phase: "ready", data: action.result.branches },
      status: { phase: "ready", data: action.result.status },
      operation: { kind: "none" },
      conflicts: [],
    };
    case "branchRefs": return {
      ...state,
      repository: state.repository && { ...state.repository, branch: action.result.branch },
      branches: { phase: "ready", data: action.result.branches },
    };
    case "stash": return {
      ...state,
      status: action.result.status ? { phase: "ready", data: action.result.status } : state.status,
      operation: action.result.operation ?? state.operation,
      conflicts: action.result.conflicts ?? state.conflicts,
    };
    case "remote": return {
      ...state,
      branches: { phase: "ready", data: action.result.branches },
      repository: action.result.head && state.repository ? {
        ...state.repository,
        branch: action.result.branches.branches.find((branch) => branch.isCurrent)?.name ?? null,
        head: action.result.head,
      } : state.repository,
      status: action.result.status ? { phase: "ready", data: action.result.status } : state.status,
      operation: action.result.operation ?? state.operation,
      conflicts: action.result.conflicts ?? state.conflicts,
    };
    case "historyMutation": return {
      ...state,
      repository: state.repository && { ...state.repository, branch: action.result.branch, head: action.result.head },
      branches: { phase: "ready", data: action.result.branches },
      status: { phase: "ready", data: action.result.state.status },
      operation: action.result.state.operation,
      conflicts: action.result.state.conflicts,
    };
  }
}

export function useRepositorySession() {
  return useReducer(repositorySessionReducer, initialState);
}
