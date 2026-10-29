import { afterEach, describe, expect, test, vi } from 'vitest';
import { flushPromises, mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import * as adapter from '../src/tauri-adapter';
import MobileAccess from '../src/components/settings/MobileAccess.vue';
import { useMobileAccessStore, type MobileStatus } from '../src/stores/mobile-access';

afterEach(()=>{vi.restoreAllMocks();});
const off=():MobileStatus=>({running:false,generation:null,url:null,qrSvg:null,pending:[],devices:[]});
const on=():MobileStatus=>({running:true,generation:'fixture-generation',url:'http://192.168.1.2:22358/#pair=fixture',qrSvg:'<svg xmlns="http://www.w3.org/2000/svg"/>',pending:[{id:'phone',name:'安卓手机',ip:'192.168.1.3'}],devices:[]});

describe('desktop mobile access controls',()=>{
  test('starts disabled, handles approval/revocation, and keeps serving after closing settings',async()=>{
    const pinia=createPinia();setActivePinia(pinia);let current=off();const unlisten=vi.fn();
    vi.spyOn(adapter,'listen').mockResolvedValue(unlisten);
    const invoke=vi.spyOn(adapter,'invoke').mockImplementation(async(command,args)=>{
      if(command==='start_mobile_access')current=on();
      if(command==='decide_mobile_device') {expect(args).toEqual({generation:'fixture-generation',id:'phone',allow:true});current.devices=[...current.pending];current.pending=[];}
      if(command==='revoke_mobile_device'){expect(args).toEqual({generation:'fixture-generation',id:'phone'});current.devices=[];}
      return structuredClone(current) as never;
    });
    const wrapper=mount(MobileAccess,{global:{plugins:[pinia]}});await flushPromises();
    expect((wrapper.get('[role=switch]').element as HTMLInputElement).checked).toBe(false);
    expect(invoke).not.toHaveBeenCalledWith('start_mobile_access',undefined);
    await wrapper.get('[role=switch]').setValue(true);await flushPromises();
    expect(wrapper.find('[aria-label="手机访问配对二维码"]').exists()).toBe(true);
    expect(wrapper.text()).toContain('安卓手机');
    await wrapper.findAll('button').find(b=>b.text()==='允许连接')!.trigger('click');await flushPromises();
    expect(wrapper.find('[aria-label="断开安卓手机"]').exists()).toBe(true);
    await wrapper.get('[aria-label="断开安卓手机"]').trigger('click');await flushPromises();
    expect(current.devices).toEqual([]);
    wrapper.unmount();
    expect(invoke.mock.calls.some(([command])=>command==='stop_mobile_access')).toBe(false);
    useMobileAccessStore().$dispose();expect(unlisten).toHaveBeenCalledOnce();
  });

  test('a late status request cannot overwrite a more recent start result',async()=>{
    setActivePinia(createPinia());vi.spyOn(adapter,'listen').mockResolvedValue(vi.fn());
    let resolve!:(value:MobileStatus)=>void;
    vi.spyOn(adapter,'invoke').mockImplementation(command=>command==='mobile_access_status'?new Promise(r=>{resolve=r as typeof resolve;}):Promise.resolve(on()) as never);
    const store=useMobileAccessStore();const refresh=store.refresh();await store.setEnabled(true);
    resolve(off());await refresh;expect(store.status.running).toBe(true);store.$dispose();
  });

  test('native diagnostic text is not shown as a user-facing error',async()=>{
    const pinia=createPinia();setActivePinia(pinia);vi.spyOn(adapter,'listen').mockResolvedValue(vi.fn());
    vi.spyOn(adapter,'invoke').mockImplementation(async command=>{if(command==='start_mobile_access')throw new Error('C:\\private\\native-stack');return off() as never;});
    const wrapper=mount(MobileAccess,{global:{plugins:[pinia]}});await flushPromises();await wrapper.get('[role=switch]').setValue(true);await flushPromises();
    expect(wrapper.get('[role=alert]').text()).toContain('操作未完成');expect(wrapper.text()).not.toContain('private');
    wrapper.unmount();useMobileAccessStore().$dispose();
  });
});
