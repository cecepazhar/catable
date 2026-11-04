import { describe, it, expect } from 'vitest';
import { portal } from './portal';

describe('portal action', () => {
  it('moves the node to document.body and removes it on destroy', () => {
    const wrapper = document.createElement('main');
    const modal = document.createElement('div');
    wrapper.appendChild(modal);
    document.body.appendChild(wrapper);

    const action = portal(modal);
    expect(modal.parentElement).toBe(document.body);
    expect(wrapper.contains(modal)).toBe(false);

    action.destroy();
    expect(document.body.contains(modal)).toBe(false);
    wrapper.remove();
  });

  it('accepts an explicit target element', () => {
    const host = document.createElement('div');
    const node = document.createElement('span');
    document.body.append(host, node);

    const action = portal(node, host);
    expect(node.parentElement).toBe(host);

    action.destroy();
    host.remove();
  });
});
