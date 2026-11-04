/**
 * Svelte action: moves the element to `document.body` (or `target`).
 *
 * Needed for modals rendered inside `<main class="relative z-10">`: that element creates its own
 * stacking context, so a `z-50` modal inside it can never paint above siblings of `<main>` (the app
 * sidebar). Mounting on <body> makes `z-50` global again.
 */
export function portal(node: HTMLElement, target: HTMLElement | string = 'body') {
  const resolve = (t: HTMLElement | string): HTMLElement =>
    typeof t === 'string' ? (document.querySelector<HTMLElement>(t) ?? document.body) : t;

  resolve(target).appendChild(node);

  return {
    destroy() {
      node.parentNode?.removeChild(node);
    },
  };
}
