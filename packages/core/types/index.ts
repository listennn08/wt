export type WorktreeInfo = {
  path: string;
  head?: string;
  branch?: string;
  detached?: boolean;
  locked?: boolean;
  prunable?: boolean;
};
