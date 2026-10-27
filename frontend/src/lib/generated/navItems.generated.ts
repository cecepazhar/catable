export interface GeneratedNavItem {
  key: string;
  label: string;
  href: string;
  icon: string;
  description: string;
  path: string;
}

export const GENERATED_NAV_ITEMS: GeneratedNavItem[] = [
  {
    key: "connections",
    label: "Connections",
    href: "/",
    icon: "plug",
    description: "Database connection profiles & vault",
    path: "M13 10V3L4 14h7v7l9-11h-7z"
  },
  {
    key: "workspaces",
    label: "Workspaces",
    href: "/groups",
    icon: "folder",
    description: "Database workspaces & clusters",
    path: "M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z"
  },
  {
    key: "sql_studio",
    label: "SQL Studio",
    href: "/sql-studio",
    icon: "terminal-square",
    description: "Multi-tab SQL query editor & execution runner",
    path: "M8 9l3 3-3 3m5 0h3M5 20h14a2 2 0 002-2V6a2 2 0 00-2-2H5a2 2 0 00-2 2v12a2 2 0 002 2z"
  },
  {
    key: "table_browser",
    label: "Table Browser",
    href: "/table-browser",
    icon: "table",
    description: "Virtualized table schema & live data grid",
    path: "M3 10h18M3 14h18m-9-4v8m-7 4h14a2 2 0 002-2V6a2 2 0 00-2-2H5a2 2 0 00-2 2v12a2 2 0 002 2z"
  },
  {
    key: "schema_erd",
    label: "Schema ERD",
    href: "/schema-erd",
    icon: "network",
    description: "Interactive database entity relationship diagrams",
    path: "M4 6a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2H6a2 2 0 01-2-2V6z M14 6a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2h-2a2 2 0 01-2-2V6z M9 16a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2h-2a2 2 0 01-2-2v-2z"
  },
  {
    key: "ai_copilot",
    label: "AI Copilot",
    href: "/ai-copilot",
    icon: "sparkles",
    description: "Schema-aware DBA copilot, NL-to-SQL & query optimizer",
    path: "M9.813 15.904L9 18.75l-.813-2.846a4.5 4.5 0 00-3.09-3.09L2.25 12l2.846-.813a4.5 4.5 0 003.09-3.09L9 5.25l.813 2.846a4.5 4.5 0 003.09 3.09L15.75 12l-2.846.813a4.5 4.5 0 00-3.09 3.09z"
  }
];
