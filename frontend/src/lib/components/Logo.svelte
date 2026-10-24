<script lang="ts">
  export let size: number | string = 28;
  export let mode: 'light' | 'dark' | 'auto' | 'brand' | 'white' = 'brand';
  export let className: string = '';
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 512 512"
  class="shrink-0 {className}"
  xmlns="http://www.w3.org/2000/svg"
>
  <!-- CATable database / table icon -->
  <!-- Blue database cylinder with two horizontal layer lines -->
  <g fill="none">
    <!-- top ellipse -->
    <ellipse
      cx="256" cy="135" rx="125" ry="36"
      stroke={mode === 'white' ? '#FFFFFF' : mode === 'light' ? '#0A0A0C' : '#60A5FA'}
      stroke-width="28"
    />
    <!-- sides and bottom -->
    <path
      d="M131,135 L131,360 A 125 36 0 0 0 381 360 L381,135"
      stroke={mode === 'white' ? '#FFFFFF' : mode === 'light' ? '#0A0A0C' : '#3B82F6'}
      stroke-width="28"
      stroke-linecap="round"
      stroke-linejoin="round"
    />
    <!-- middle layer lines -->
    <path
      d="M131,210 A 125 36 0 0 0 381 210"
      stroke={mode === 'white' ? '#FFFFFF' : mode === 'light' ? '#0A0A0C' : '#60A5FA'}
      stroke-width="28"
      stroke-linecap="round"
    />
    <path
      d="M131,285 A 125 36 0 0 0 381 285"
      stroke={mode === 'white' ? '#FFFFFF' : mode === 'light' ? '#0A0A0C' : '#60A5FA'}
      stroke-width="28"
      stroke-linecap="round"
    />
  </g>
</svg>
