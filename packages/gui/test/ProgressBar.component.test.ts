import { describe, expect, test, vi } from 'vitest';
import { mount } from '@vue/test-utils';
import ProgressBar from '../src/components/player/ProgressBar.vue';

function mountProgress() {
  const wrapper = mount(ProgressBar, {
    props: {
      currentTime: 20,
      duration: 100,
      currentTimeStr: '0:20',
      durationStr: '1:40',
      bufferedStyle: { transform: 'scaleX(0.5)' },
      video: null,
      match: null,
    },
  });
  const track = wrapper.find('.player-progress-track').element as HTMLElement;
  track.getBoundingClientRect = () => ({
    x: 10,
    y: 0,
    left: 10,
    right: 210,
    top: 0,
    bottom: 4,
    width: 200,
    height: 4,
    toJSON: () => ({}),
  });
  return wrapper;
}

describe('ProgressBar drag lifecycle', () => {
  test('reports start, continuous seeks, and end to the player host', async () => {
    const wrapper = mountProgress();
    await wrapper.find('.player-progress-wrap').trigger('pointerdown', {
      button: 0,
      buttons: 1, pointerId: 1, isPrimary: true,
      clientX: 50,
    });
    document.dispatchEvent(new PointerEvent('pointermove', { clientX: 150, buttons: 1, pointerId: 1 }));
    document.dispatchEvent(new PointerEvent('pointerup', { clientX: 150, pointerId: 1 }));

    expect(wrapper.emitted('seekStart')).toHaveLength(1);
    expect(wrapper.emitted('seekEnd')).toHaveLength(1);
    expect(wrapper.emitted('seek')).toEqual([[0.2], [0.7], [0.7]]);
    wrapper.unmount();
  });

  test('ends the drag and removes global listeners when unmounted', async () => {
    const removeListener = vi.spyOn(document, 'removeEventListener');
    const wrapper = mountProgress();
    await wrapper.find('.player-progress-wrap').trigger('pointerdown', {
      button: 0,
      buttons: 1, pointerId: 1, isPrimary: true,
      clientX: 50,
    });
    wrapper.unmount();
    document.dispatchEvent(new PointerEvent('pointermove', { clientX: 150, buttons: 1, pointerId: 1 }));

    expect(wrapper.emitted('seekEnd')).toHaveLength(1);
    expect(removeListener).toHaveBeenCalledWith('pointermove', expect.any(Function));
    expect(removeListener).toHaveBeenCalledWith('pointerup', expect.any(Function));
    removeListener.mockRestore();
  });

  test('ignores pointer input while the responsive track has zero width', async () => {
    const wrapper = mountProgress();
    const track = wrapper.get('.player-progress-track').element as HTMLElement;
    track.getBoundingClientRect = () => ({
      x: 10,
      y: 0,
      left: 10,
      right: 10,
      top: 0,
      bottom: 4,
      width: 0,
      height: 4,
      toJSON: () => ({}),
    });

    await wrapper.get('.player-progress-wrap').trigger('pointerdown', {
      button: 0,
      buttons: 1, pointerId: 1, isPrimary: true,
      clientX: 10,
    });

    expect(wrapper.emitted('seekStart')).toBeUndefined();
    expect(wrapper.emitted('seek')).toBeUndefined();
    wrapper.unmount();
  });

  test('clamps visual and ARIA state to finite media bounds', async () => {
    const wrapper = mountProgress();
    await wrapper.setProps({ currentTime: 250, duration: 100 });
    const slider = wrapper.get('.player-progress-wrap');
    expect(slider.attributes('aria-valuenow')).toBe('100');
    expect(slider.attributes('aria-valuemax')).toBe('100');
    expect(wrapper.get('.player-progress-fill').attributes('style')).toContain('scaleX(1)');
    expect(wrapper.get('.player-progress-thumb').attributes('style')).toContain('left: 100%');

    await wrapper.setProps({ currentTime: Number.POSITIVE_INFINITY, duration: Number.NaN });
    expect(slider.attributes('aria-valuenow')).toBe('0');
    expect(slider.attributes('aria-valuemax')).toBe('0');
    expect(wrapper.get('.player-progress-fill').attributes('style')).toContain('scaleX(0)');
    wrapper.unmount();
  });

  test('drag visuals follow the pointer even if decoded time is stale, and release commits its final coordinate', async () => {
    const wrapper = mountProgress();
    const slider = wrapper.get('.player-progress-wrap');
    await slider.trigger('pointerdown', { button: 0, buttons: 1, pointerId: 7, isPrimary: true, clientX: 50 });
    document.dispatchEvent(new PointerEvent('pointermove', { pointerId: 7, buttons: 1, clientX: 170 }));
    await wrapper.vm.$nextTick();
    expect(wrapper.get('.player-progress-thumb').attributes('style')).toContain('left: 80%');
    expect(slider.attributes('aria-valuenow')).toBe('80');
    expect(wrapper.get('.player-progress-preview').text()).toBe('1:20');
    await wrapper.setProps({ currentTime: 21 });
    expect(slider.attributes('aria-valuenow')).toBe('80');
    document.dispatchEvent(new PointerEvent('pointerup', { pointerId: 7, clientX: 210 }));
    expect(wrapper.emitted('seek')?.at(-1)).toEqual([1]);
    expect(wrapper.emitted('seekEnd')).toHaveLength(1);
    wrapper.unmount();
  });

  test('lost capture, cancellation and missing mouse release each leave the next drag usable', async () => {
    const wrapper = mountProgress();
    const slider = wrapper.get('.player-progress-wrap');
    for (const kind of ['lostpointercapture', 'pointercancel', 'missing-release', 'blur']) {
      await slider.trigger('pointerdown', { button: 0, buttons: 1, pointerId: 3, isPrimary: true, clientX: 100 });
      // A different finger cannot end the primary drag.
      document.dispatchEvent(new PointerEvent('pointerup', { pointerId: 4, clientX: 0 }));
      expect(slider.classes()).toContain('is-dragging');
      if (kind === 'lostpointercapture') await slider.trigger(kind, { pointerId: 3 });
      else if (kind === 'pointercancel') document.dispatchEvent(new PointerEvent(kind, { pointerId: 3 }));
      else if (kind === 'blur') window.dispatchEvent(new Event('blur'));
      else document.dispatchEvent(new PointerEvent('pointermove', { pointerId: 3, pointerType: 'mouse', buttons: 0, clientX: 130 }));
      await wrapper.vm.$nextTick();
      expect(slider.classes()).not.toContain('is-dragging');
    }
    expect(wrapper.emitted('seekStart')).toHaveLength(4);
    expect(wrapper.emitted('seekEnd')).toHaveLength(4);
    wrapper.unmount();
  });

  test('ignores unloaded duration and resets drag on video replacement', async () => {
    const wrapper = mountProgress();
    const slider = wrapper.get('.player-progress-wrap');
    await wrapper.setProps({ duration: 0 });
    await slider.trigger('pointerdown', { button: 0, pointerId: 1, isPrimary: true, clientX: 100 });
    expect(wrapper.emitted('seek')).toBeUndefined();
    await wrapper.setProps({ duration: 100 });
    await slider.trigger('pointerdown', { button: 0, buttons: 1, pointerId: 1, isPrimary: true, clientX: 100 });
    await wrapper.setProps({ video: { video_id: 'replacement' } as never });
    expect(wrapper.emitted('seekEnd')).toHaveLength(1);
    expect(slider.classes()).not.toContain('is-dragging');
    wrapper.unmount();
  });

  test('keyboard seeking still clamps to media limits', async () => {
    const wrapper = mountProgress();
    const slider = wrapper.get('.player-progress-wrap');
    await slider.trigger('keydown', { key: 'ArrowRight' });
    await slider.trigger('keydown', { key: 'Home' });
    await slider.trigger('keydown', { key: 'End' });
    expect(wrapper.emitted('seek')).toEqual([[0.25], [0], [1]]);
    wrapper.unmount();
  });

  test('event markers keep click and keyboard jumps separate from track dragging', async () => {
    const wrapper = mountProgress();
    await wrapper.setProps({
      match: { agent: { agent_name: 'Jett' }, gameStartTime: '2026-01-01 12:00:00', gameEndTime: '2026-01-01 12:10:00' } as never,
      video: { video_id: 'fixture', video_type: '击杀集锦', video_duration: 100000,
        rounds: [{ round_clips: [{ clip_events: [{ event_type: 'kill', event_sTime: 5000,
          event_ext: { EventName: 'Shot', EventTime: '2026-01-01 12:00:05', AgentName: 'Jett',
            KillerPlayerName: 'Actor', KilledPlayerName: 'Opponent', KillerIsMe: 1, KilledIsMe: 0, GetShotRolePart: 1 } }] }] }] } as never,
    });
    const marker = wrapper.get('.player-event-marker');
    await marker.trigger('pointerdown', { button: 0, pointerId: 1, isPrimary: true, clientX: 20 });
    await marker.trigger('click');
    await marker.trigger('keydown', { key: 'Enter' });
    expect(wrapper.emitted('seekStart')).toBeUndefined();
    expect(wrapper.emitted('markerClick')).toEqual([[5000], [5000]]);
    wrapper.unmount();
  });
});
