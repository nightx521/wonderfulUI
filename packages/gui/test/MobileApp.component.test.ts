import { afterEach,beforeEach,describe,expect,test,vi } from 'vitest';
import { mount,flushPromises } from '@vue/test-utils';
import MobileApp from '../../mobile/src/App.vue';
import type { MatchRecord } from '@wonderful-ui/parser';

function fixture(){
  const match:MatchRecord={matches_id:'opaque-match',openID:'opaque-account',matches_time:1781956800000,
    map:{map_id:'/Game/Maps/Ascent/Ascent'},agent:{agent_id:'',agent_name:'Jett'},mode:'competitive',
    stats:{kills:12,deaths:5,assists:3,score:200,has_won:true,result_known:true,rounds_won:13,rounds_lost:5,game_level:'',mode_name:''},
    minRoundId:0,gameStartTime:'2026-06-20 12:00:00',gameEndTime:'2026-06-20 12:30:00',videos:[]};
  return {accounts:[{openid:'opaque-account',nick:'Fixture account'}],matches:[match],totalErrors:0};
}
const json=(data:unknown,status=200)=>new Response(JSON.stringify(data),{status,headers:{'Content-Type':'application/json'}});
beforeEach(()=>{vi.useFakeTimers();history.replaceState(null,'','/');sessionStorage.clear();
  Object.defineProperty(HTMLDialogElement.prototype,'showModal',{configurable:true,value:function(this:HTMLDialogElement){this.setAttribute('open','');}});
  Object.defineProperty(HTMLDialogElement.prototype,'close',{configurable:true,value:function(this:HTMLDialogElement){this.removeAttribute('open');}});
});
afterEach(()=>{vi.useRealTimers();vi.unstubAllGlobals();vi.restoreAllMocks();history.replaceState(null,'','/');});
describe('mobile library browser',()=>{
  test('hides the library until approved and removes invitation tokens from the address bar',async()=>{
    history.replaceState(null,'','/#pair=fixture-invite');let approved=false;
    const fetcher=vi.fn(async(input:RequestInfo|URL)=>{
      const url=String(input);if(url==='/api/library')return approved?json(fixture()):json({message:'Not paired'},401);
      if(url==='/api/pair')return json({requestId:'pending',requestToken:'private-poll-secret'});
      if(url==='/api/pair/status')return json({status:approved?'approved':'pending'});
      return json({connected:true});
    });vi.stubGlobal('fetch',fetcher);
    const wrapper=mount(MobileApp);await flushPromises();
    expect(location.hash).toBe('');expect(wrapper.text()).not.toContain('Fixture account');
    await wrapper.get('form').trigger('submit');await flushPromises();expect(wrapper.text()).toContain('等待电脑确认');
    await vi.advanceTimersByTimeAsync(1500);await flushPromises();expect(wrapper.text()).not.toContain('Fixture account');
    approved=true;await vi.advanceTimersByTimeAsync(1500);await flushPromises();
    expect(wrapper.text()).toContain('对局回放');expect(wrapper.text()).toContain('Fixture account');
    expect(wrapper.html()).not.toContain('private-poll-secret');wrapper.unmount();
  });
  test('keeps partial decimal input as a draft and clears data after revocation',async()=>{
    let connected=true;
    vi.stubGlobal('fetch',vi.fn(async(input:RequestInfo|URL)=>String(input)==='/api/library'?json(fixture()):connected?json({connected:true}):json({message:'连接已失效，请重新扫码'},401)));
    const wrapper=mount(MobileApp);await flushPromises();
    await wrapper.findAll('button').find(b=>b.text()==='筛选')!.trigger('click');
    const input=wrapper.get('input[aria-label="KDA最小值"]');await input.setValue('0.');expect((input.element as HTMLInputElement).value).toBe('0.');
    await input.setValue('0.5');await wrapper.findAll('button').find(b=>b.text()==='查看结果')!.trigger('click');await flushPromises();
    expect(wrapper.findAll('.match-card')).toHaveLength(1);
    connected=false;await vi.advanceTimersByTimeAsync(1500);await flushPromises();
    expect(wrapper.text()).not.toContain('Fixture account');expect(wrapper.text()).toContain('连接已失效');wrapper.unmount();
  });
});
