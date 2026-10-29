import { afterEach, expect, test, vi } from 'vitest';
import { flushPromises, mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import * as adapter from '../src/tauri-adapter';
import RecordWarningConfirm from '../src/components/common/RecordWarningConfirm.vue';
import { useAccountStore } from '../src/stores/account';
afterEach(()=>{vi.restoreAllMocks();document.body.innerHTML='';});
test('only confirms the reviewed revision after an explicit second confirmation',async()=>{
  setActivePinia(createPinia());vi.spyOn(useAccountStore(),'loadLibrary').mockResolvedValue(undefined);
  const invoke=vi.spyOn(adapter,'invoke').mockResolvedValue(undefined);
  const props={openid:'fixture-account',expected:'{"skipped_records":1}',label:'Fixture',position:{x:10,y:10}};
  const wrapper=mount(RecordWarningConfirm,{props,attachTo:document.body});await flushPromises();
  expect(document.querySelector('[role=alertdialog]')!.textContent).toContain('原始数据和视频都会保留');
  expect(invoke).not.toHaveBeenCalled();
  document.querySelector<HTMLButtonElement>('.record-warning-actions .btn-primary')!.click();await flushPromises();
  expect(invoke).toHaveBeenCalledWith('acknowledge_record_warnings',{openid:props.openid,expected:props.expected});
  expect(wrapper.emitted('close')).toHaveLength(1);wrapper.unmount();
});
