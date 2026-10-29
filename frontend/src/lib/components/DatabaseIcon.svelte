<script lang="ts">
  interface Props {
    driver: string;
    size?: number;
    class?: string;
    withBadge?: boolean;
    badgeSize?: 'sm' | 'md' | 'lg';
  }

  let {
    driver = 'sqlite',
    size = 20,
    class: className = '',
    withBadge = false,
    badgeSize = 'md'
  }: Props = $props();

  const d = $derived((driver || 'sqlite').toLowerCase());

  // Authentic driver metadata: theme colors and wireframe strokes
  const driverMeta = $derived.by(() => {
    switch (d) {
      case 'postgres':
      case 'postgresql':
        return {
          name: 'PostgreSQL',
          short: 'PG',
          color: 'text-sky-400',
          strokeColor: '#38bdf8',
          borderClass: 'border-sky-500/30 bg-sky-500/10 text-sky-400 shadow-[0_0_10px_rgba(56,189,248,0.15)]',
          glow: 'rgba(56,189,248,0.2)'
        };
      case 'mysql':
        return {
          name: 'MySQL',
          short: 'MY',
          color: 'text-cyan-400',
          strokeColor: '#22d3ee',
          borderClass: 'border-cyan-500/30 bg-cyan-500/10 text-cyan-400 shadow-[0_0_10px_rgba(34,211,238,0.15)]',
          glow: 'rgba(34,211,238,0.2)'
        };
      case 'sqlite':
        return {
          name: 'SQLite',
          short: 'SQ',
          color: 'text-blue-400',
          strokeColor: '#60a5fa',
          borderClass: 'border-blue-500/30 bg-blue-500/10 text-blue-400 shadow-[0_0_10px_rgba(96,165,250,0.15)]',
          glow: 'rgba(96,165,250,0.2)'
        };
      case 'mariadb':
        return {
          name: 'MariaDB',
          short: 'MA',
          color: 'text-amber-400',
          strokeColor: '#fbbf24',
          borderClass: 'border-amber-500/30 bg-amber-500/10 text-amber-400 shadow-[0_0_10px_rgba(251,191,36,0.15)]',
          glow: 'rgba(251,191,36,0.2)'
        };
      case 'redis':
        return {
          name: 'Redis',
          short: 'RD',
          color: 'text-rose-400',
          strokeColor: '#fb7185',
          borderClass: 'border-rose-500/30 bg-rose-500/10 text-rose-400 shadow-[0_0_10px_rgba(251,113,133,0.15)]',
          glow: 'rgba(251,113,133,0.2)'
        };
      case 'duckdb':
        return {
          name: 'DuckDB',
          short: 'DK',
          color: 'text-yellow-400',
          strokeColor: '#facc15',
          borderClass: 'border-yellow-500/30 bg-yellow-500/10 text-yellow-400 shadow-[0_0_10px_rgba(250,204,21,0.15)]',
          glow: 'rgba(250,204,21,0.2)'
        };
      case 'mongodb':
      case 'mongo':
        return {
          name: 'MongoDB',
          short: 'MG',
          color: 'text-emerald-400',
          strokeColor: '#34d399',
          borderClass: 'border-emerald-500/30 bg-emerald-500/10 text-emerald-400 shadow-[0_0_10px_rgba(52,211,153,0.15)]',
          glow: 'rgba(52,211,153,0.2)'
        };
      case 'mssql':
      case 'sqlserver':
        return {
          name: 'SQL Server',
          short: 'MS',
          color: 'text-red-400',
          strokeColor: '#f87171',
          borderClass: 'border-red-500/30 bg-red-500/10 text-red-400 shadow-[0_0_10px_rgba(248,113,113,0.15)]',
          glow: 'rgba(248,113,113,0.2)'
        };
      case 'clickhouse':
        return {
          name: 'ClickHouse',
          short: 'CH',
          color: 'text-orange-400',
          strokeColor: '#fb923c',
          borderClass: 'border-orange-500/30 bg-orange-500/10 text-orange-400 shadow-[0_0_10px_rgba(251,146,60,0.15)]',
          glow: 'rgba(251,146,60,0.2)'
        };
      default:
        return {
          name: 'Database',
          short: 'DB',
          color: 'text-neutral-400',
          strokeColor: '#9ca3af',
          borderClass: 'border-neutral-500/30 bg-neutral-500/10 text-neutral-400',
          glow: 'transparent'
        };
    }
  });

  const badgeDims = $derived.by(() => {
    switch (badgeSize) {
      case 'sm':
        return 'w-7 h-7 rounded-lg';
      case 'lg':
        return 'w-11 h-11 rounded-2xl';
      case 'md':
      default:
        return 'w-9 h-9 rounded-xl';
    }
  });
</script>

{#snippet iconContent()}
  <svg
    width={size}
    height={size}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="1.6"
    stroke-linecap="round"
    stroke-linejoin="round"
    class="{driverMeta.color} {className} shrink-0 transition-transform duration-200"
    aria-label={driverMeta.name}
  >
    {#if d === 'postgres' || d === 'postgresql'}
      <!-- AUTHENTIC POSTGRESQL ELEPHANT (SLONIK) MONOLINE WIREFRAME -->
      <!-- Head dome & Crown -->
      <path d="M12 3.5C8 3.5 5.5 6 5.5 9.5C5.5 12 7 13.8 8.5 14.8" />
      <!-- Left Large Ear Curve -->
      <path d="M5.5 7.5C3.5 8 2.5 10 2.5 12C2.5 14.5 4 16 6 16C7 16 7.8 15.5 8.5 14.8" />
      <!-- Trunk curving down and back up with hollow tip -->
      <path d="M10 13.5V17C10 19 11.5 20.5 13.5 20.5C14.8 20.5 15.8 19.8 16 18.5C16 17.5 15.2 16.8 14.2 16.8" />
      <!-- Right Forehead and Cheek -->
      <path d="M12 3.5C16 3.5 18.5 6 18.5 9.5C18.5 11.5 17.5 13.2 16 14.2" />
      <!-- Right Ear -->
      <path d="M18.5 7.5C20.5 8 21.5 10 21.5 12C21.5 14.5 20 16 18 16C17 16 16.2 15.5 15.5 14.8" />
      <!-- Authentic Sharp Curved Tusks -->
      <path d="M7.5 14.5C6.5 16 5.5 17.5 4.5 18C4 18.2 4.5 16.5 6 14" />
      <path d="M14.5 14.5C15.5 16 16.5 17.5 17.5 18C18 18.2 17.5 16.5 16 14" />
      <!-- Eyes (precision hollow circles) -->
      <circle cx="9" cy="9.5" r="0.9" fill="currentColor" />
      <circle cx="15" cy="9.5" r="0.9" fill="currentColor" />

    {:else if d === 'mysql'}
      <!-- AUTHENTIC MYSQL DOLPHIN (SAKILA) LEAPING MONOLINE WIREFRAME -->
      <!-- Snout & Beak -->
      <path d="M2.5 13C3.5 12.2 4.8 11.8 6 11.8" />
      <!-- Arched Leaping Head & Back -->
      <path d="M6 11.8C8 7.5 12.5 5 17 6.2C19.5 6.9 21.2 8.5 21.5 10.5" />
      <!-- Dorsal Fin -->
      <path d="M13.5 6.2C14.2 4 16 3 17 3C16.8 4.5 16 5.8 15.2 6.5" />
      <!-- Lower Jaw & Belly -->
      <path d="M2.5 13C4 13.8 5.8 14.2 7.5 14C11 13.8 14 11.5 16.5 12C18.5 12.4 20.2 13.8 21.5 15" />
      <!-- Tail Flukes (Classic Sakila Dual Fin) -->
      <path d="M21.5 10.5C21 12 21 13.8 21.5 15C22.2 14.2 23 13 23.5 11.5C23.2 11.2 22.2 11 21.5 10.5Z" />
      <path d="M21.5 15C22.2 16.2 23 17 23.5 17.5C23 16.5 22.2 15.5 21.5 15Z" />
      <!-- Pectoral Flipper -->
      <path d="M8.5 13.8C8 16 7.5 17.5 6.5 18.5C8 18 9.5 16.5 10.5 14.5" />
      <!-- Eye -->
      <circle cx="7.2" cy="10" r="0.8" fill="currentColor" />

    {:else if d === 'sqlite'}
      <!-- AUTHENTIC SQLITE FEATHER QUILL & DATABASE VAULT WIREFRAME -->
      <!-- Database Cylinder Top Ellipse -->
      <ellipse cx="12" cy="6" rx="8" ry="3" />
      <!-- Cylindrical Walls with Layer Cutouts -->
      <path d="M4 6V16C4 17.66 7.58 19 12 19C16.42 19 20 17.66 20 16V6" />
      <path d="M4 11C4 12.66 7.58 14 12 14C16.42 14 20 12.66 20 11" />
      <!-- Iconic SQLite Slicing Quill Feather Spine -->
      <path d="M19 3L11 15L7.5 14L15.5 2L19 3Z" stroke-width="1.2" />
      <path d="M11 15L9.5 18L12 16.5" stroke-width="1.2" />
      <path d="M14 6L16.5 7.5M12.5 9L15 10.5" stroke-width="1.2" />

    {:else if d === 'mariadb'}
      <!-- AUTHENTIC MARIADB SEA SEAL (MARINA) MONOLINE WIREFRAME -->
      <!-- Seal Snout & Whiskers Area -->
      <path d="M3 10.5C3.8 9.5 5 9 6.2 9.2C7.5 9.5 8.2 10.5 8.5 11.5" />
      <path d="M3 10.5C3.5 11.8 4.5 12.5 6 12.5" />
      <!-- Whiskers -->
      <path d="M4 11.5L2 12M4 12.2L2.2 13.5" stroke-width="1.2" />
      <!-- Arched Swimming Body -->
      <path d="M6.2 9.2C8 6 12 4.5 16 5.5C19 6.5 21 8.5 21.5 11C21.8 13.5 20.5 16 19 17.5" />
      <!-- Underbelly & Tail -->
      <path d="M6 12.5C9 13.5 12 12 14.5 13C17 14 18 16 19 17.5" />
      <!-- Tail Flippers -->
      <path d="M19 17.5C20.5 18.5 22 19.5 22.5 20.5C21 19.8 19.8 19 19 17.5Z" />
      <path d="M19 17.5C18.2 19 17.5 20.5 17 21.5C17.8 20 18.5 18.8 19 17.5Z" />
      <!-- Front Flipper -->
      <path d="M9.5 12.5C9 14.5 8 16.2 6.8 17.5C8.5 16.8 10 15 11 13" />
      <!-- Eye -->
      <circle cx="6.5" cy="10" r="0.8" fill="currentColor" />

    {:else if d === 'redis'}
      <!-- AUTHENTIC REDIS ISOMETRIC 3-TIER MEMORY LATTICE -->
      <!-- Top Diamond Layer -->
      <path d="M12 2.5L20 6.5L12 10.5L4 6.5L12 2.5Z" />
      <!-- Middle Layer -->
      <path d="M4 10.5L12 14.5L20 10.5" />
      <path d="M4 11L4 12L12 16L20 12L20 11" stroke-width="1.2" />
      <!-- Bottom Layer -->
      <path d="M4 15.5L12 19.5L20 15.5" />
      <path d="M4 16L4 17.5L12 21.5L20 17.5L20 16" stroke-width="1.2" />
      <!-- Center Core Pulse Beam -->
      <path d="M12 4.5V18.5" stroke-dasharray="1.5 2" stroke-width="1.2" />

    {:else if d === 'duckdb'}
      <!-- AUTHENTIC DUCKDB DUCK SILHOUETTE MONOLINE WIREFRAME -->
      <!-- Duck Bill / Beak -->
      <path d="M2.5 10.5C4 9.5 6 9.5 7.5 10C8 9 9 7 11 6C13.5 4.8 16.5 5.5 18 7.5" />
      <path d="M2.5 10.5C4.2 11.2 6.5 11.5 8 11.2" />
      <!-- Crown, Neck & Breast -->
      <path d="M18 7.5C19.5 9.5 19.8 12 19 14.5C17.8 18 14 20.5 10 20.5C6 20.5 4 18 4 15.5C4 13.8 5.5 12.2 8 11.2" />
      <!-- Tail & Wing Sweep -->
      <path d="M19 14.5C20.5 14 22 13 22.5 12C22 13.8 20.5 16 19 17.5" />
      <path d="M9 14C11.5 13.5 14.5 14.2 16 16" />
      <!-- Eye -->
      <circle cx="12" cy="8.5" r="1" fill="currentColor" />

    {:else if d === 'mongodb' || d === 'mongo'}
      <!-- AUTHENTIC MONGODB LEAF / FLAME MONOLINE WIREFRAME -->
      <!-- Left Outer Leaf Curve -->
      <path d="M12 2C8 6 5.5 10.5 5.5 15C5.5 18.5 8.2 21.5 12 22" />
      <!-- Right Outer Leaf Curve -->
      <path d="M12 2C16 6 18.5 10.5 18.5 15C18.5 18.5 15.8 21.5 12 22" />
      <!-- Center Leaf Stem & Spine -->
      <path d="M12 2V22" />
      <!-- Inner Vein Accents -->
      <path d="M12 7C9.5 10 8.5 13 8.5 16" stroke-width="1.2" />
      <path d="M12 7C14.5 10 15.5 13 15.5 16" stroke-width="1.2" />

    {:else if d === 'mssql' || d === 'sqlserver'}
      <!-- AUTHENTIC SQL SERVER 4-TIER SEGMENTED CYLINDER MATRIX -->
      <ellipse cx="12" cy="5" rx="8" ry="2.5" />
      <path d="M4 5V19C4 20.38 7.58 21.5 12 21.5C16.42 21.5 20 20.38 20 19V5" />
      <path d="M4 9.5C4 10.88 7.58 12 12 12C16.42 12 20 10.88 20 9.5" />
      <path d="M4 14.2C4 15.58 7.58 16.7 12 16.7C16.42 16.7 20 15.58 20 14.2" />
      <!-- Vertical Matrix Partition -->
      <path d="M12 7.5V21" stroke-dasharray="1.5 1.5" stroke-width="1.2" />

    {:else if d === 'clickhouse'}
      <!-- AUTHENTIC CLICKHOUSE EQUALIZER DATA BARS TOWER -->
      <rect x="3" y="11" width="2" height="9" rx="0.8" />
      <rect x="7" y="7" width="2" height="13" rx="0.8" />
      <rect x="11" y="4" width="2" height="16" rx="0.8" />
      <rect x="15" y="8" width="2" height="12" rx="0.8" />
      <rect x="19" y="13" width="2" height="7" rx="0.8" />
      <path d="M2 20.5H22" stroke-width="1.2" />

    {:else}
      <!-- GENERIC DATABASE VAULT WIREFRAME -->
      <ellipse cx="12" cy="5" rx="8" ry="3" />
      <path d="M4 5V19C4 20.66 7.58 22 12 22C16.42 22 20 20.66 20 19V5" />
      <path d="M4 12C4 13.66 7.58 15 12 15C16.42 15 20 13.66 20 12" />
      <circle cx="8" cy="8.5" r="0.8" fill="currentColor" />
      <circle cx="8" cy="15.5" r="0.8" fill="currentColor" />
    {/if}
  </svg>
{/snippet}

{#if withBadge}
  <div class="{badgeDims} {driverMeta.borderClass} border flex items-center justify-center shrink-0 transition-all duration-200 group-hover:scale-105">
    {@render iconContent()}
  </div>
{:else}
  {@render iconContent()}
{/if}
