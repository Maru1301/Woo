import { createContext, useContext, useLayoutEffect, useRef, type ReactNode } from "react";

const RepositoryViewActive = createContext(true);
const RepositoryViewId = createContext("");

// A mounted view retains its last complete React subtree while another
// repository is visible. Hidden panels retain DOM, selection, and scroll state;
// in-flight operations can still finish for their original repository.
export function RepositoryView({ repositoryId, active, ready, className, children }: {
  repositoryId: string;
  active: boolean;
  ready: boolean;
  className: string;
  children: ReactNode;
}) {
  const retained = useRef<ReactNode>(null);
  // Capture only committed active presentations. A concurrent render that is
  // abandoned must not replace the hidden view's last complete frame.
  useLayoutEffect(() => { if (active && children != null) retained.current = children; }, [active, children]);
  return <div className={className} hidden={!active} aria-hidden={!active}>
    <RepositoryViewId.Provider value={repositoryId}><RepositoryViewActive.Provider value={active && ready}>{active && children != null ? children : retained.current}</RepositoryViewActive.Provider></RepositoryViewId.Provider>
  </div>;
}

export function useRepositoryViewActive(): boolean {
  return useContext(RepositoryViewActive);
}

export function useRepositoryViewId(): string {
  return useContext(RepositoryViewId);
}
