export type AgentTreePhase =
  | "understanding"
  | "translating"
  | "subtitle_shaping"
  | "repairing"
  | "finalizing";

export type AgentTreeNodeKind = "root" | "tool" | "repair" | "failure";

export type AgentTreeNodeState = "idle" | "planned" | "running" | "done" | "failed";

export type AgentTreeNodeEmphasis = "" | "focus";

export type AgentTreeEdgeKind = "trunk" | "repair" | "failure";

export type AgentTreeNode = {
  key: string;
  toolName: string;
  label: string;
  kind: AgentTreeNodeKind;
  state: AgentTreeNodeState;
  phase: AgentTreePhase | "";
  detail: string;
  reason: string;
  meta: string;
  parentKey: string;
  emphasis: AgentTreeNodeEmphasis;
  order: number;
  branchIndex: number;
  revealSequence: number;
  x: number;
  y: number;
  width: number;
  height: number;
};

export type AgentTreeEdge = {
  key: string;
  from: string;
  to: string;
  kind: AgentTreeEdgeKind;
  state: "done" | "active" | "planned" | "failed";
  revealSequence: number;
};

export type AgentTreeGraph = {
  width: number;
  height: number;
  nodes: AgentTreeNode[];
  edges: AgentTreeEdge[];
  focusKey: string;
  handoffKey: string;
};
