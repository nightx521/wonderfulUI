import { afterEach, describe, expect, test, vi } from 'vitest';
import { flushPromises, mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import * as adapter from '../src/tauri-adapter';
import MobilePairPrompt from '../src/components/settings/MobilePairPrompt.vue';
import { useMobileAccessStore, type MobileStatus } from '../src/stores/mobile-access';

afterEach(()=>{vi.useRealTimers();vi.restoreAllMocks();document.body.innerHTML='';});
function fixture(){
  setActivePinia(createPinia());
  let current:MobileStatus={running:true,generation:'fixture-generation',url:null,qrSvg:null,pending:[{id:'fixture-phone',name:'安卓手机',ip:'192.168.1.3',remainingSeconds:60}],devices:[]};
  vi.spyOn(adapter,'listen').mockResolvedValue(vi.fn());
  const invoke=vi.spyOn(adapter,'invoke').mockImplementation(async(command,args)=>{
    if(command==='decide_mobile_device')current={...current,pending:[],devices:args?.allow?[{id:'fixture-phone',name:'安卓手机',ip:'192.168.1.3'}]:[]};
    return structuredClone(current) as never;
  });
  const wrapper=mount(MobilePairPrompt,{attachTo:document.body});
  return {wrapper,invoke};
}
describe('incoming mobile device prompt',()=>{
  test('appears without settings open, traps focus, and rejects automatically at sixty seconds',async()=>{
    vi.useFakeTimers();const {wrapper,invoke}=fixture();await flushPromises();
    const dialog=document.querySelector<HTMLElement>('[role=alertdialog]')!;
    expect(dialog.textContent).toContain('安卓手机');expect(document.activeElement).toBe(dialog);
    await vi.advanceTimersByTimeAsync(59000);expect(invoke.mock.calls.some(([c])=>c==='decide_mobile_device')).toBe(false);
    await vi.advanceTimersByTimeAsync(1000);await flushPromises();
    expect(invoke).toHaveBeenCalledWith('decide_mobile_device',{generation:'fixture-generation',id:'fixture-phone',allow:false});
    expect(document.querySelector('[role=alertdialog]')).toBeNull();wrapper.unmount();useMobileAccessStore().$dispose();
  });
  test('explicit approval closes the prompt and Escape rejects instead of acting on the player',async()=>{
    const {wrapper,invoke}=fixture();await flushPromises();
    document.querySelector<HTMLButtonElement>('.mobile-pair-actions .btn-primary')!.click();await flushPromises();
    expect(invoke).toHaveBeenCalledWith('decide_mobile_device',{generation:'fixture-generation',id:'fixture-phone',allow:true});
    wrapper.unmount();useMobileAccessStore().$dispose();
    const again=fixture();await flushPromises();
    document.querySelector('[role=alertdialog]')!.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}));await flushPromises();
    expect(again.invoke).toHaveBeenCalledWith('decide_mobile_device',{generation:'fixture-generation',id:'fixture-phone',allow:false});
    again.wrapper.unmount();useMobileAccessStore().$dispose();
  });
});
