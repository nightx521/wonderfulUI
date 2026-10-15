import { describe, test, expect, vi } from 'vitest';
import { mount } from '@vue/test-utils';
import { createTestingPinia } from '@pinia/testing';
import TopBar from '../src/components/layout/TopBar.vue';
import { useFilterStore } from '../src/stores/filter.ts';
import { EMPTY_FILTERS } from '../src/utils/filters.ts';

describe('TopBar library search', () => {
  test('Escape cancels pending input while preserving category filters', async () => {
    vi.useFakeTimers();
    const wrapper = mount(TopBar, {
      global: { plugins: [createTestingPinia({ createSpy: vi.fn, stubActions: false,
        initialState: { filter: { filters: { ...EMPTY_FILTERS, heroes: ['捷风'] } } },
      })] },
    });
    try {
      const filter = useFilterStore();
      const search = wrapper.get('input.search');
      expect(search.attributes('placeholder')).not.toContain('玩家');
      expect(wrapper.find('.search-mode').exists()).toBe(false);
      await search.setValue('捷风');
      await vi.advanceTimersByTimeAsync(150);
      expect(filter.filters.query).toBe('捷风');
      await search.setValue('贤者');
      await search.trigger('keydown', { key: 'Escape' });
      await vi.advanceTimersByTimeAsync(150);
      expect(filter.filters.query).toBe('');
      expect(filter.filters.heroes).toEqual(['捷风']);
    } finally {
      wrapper.unmount();
      vi.useRealTimers();
    }
  });
});
