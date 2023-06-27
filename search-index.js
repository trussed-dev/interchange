var searchIndex = JSON.parse('{\
"interchange":{"doc":"Implement a somewhat convenient and somewhat efficient way …","t":"NNNDDNDDNDNDELLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL","n":["BuildingRequest","BuildingResponse","Canceled","Channel","Error","Idle","Interchange","InterchangeRef","Requested","Requester","Responded","Responder","State","acknowledge_cancel","as_interchange_ref","borrow","borrow","borrow","borrow","borrow","borrow","borrow","borrow_mut","borrow_mut","borrow_mut","borrow_mut","borrow_mut","borrow_mut","borrow_mut","cancel","claim","claim","clone","clone","default","default","drop","drop","eq","eq","fmt","fmt","from","from","from","from","from","from","from","from","into","into","into","into","into","into","into","is_canceled","new","new","request","request","request_mut","requester","respond","responder","response","response_mut","send_request","send_response","split","state","state","take_request","take_response","try_from","try_from","try_from","try_from","try_from","try_from","try_from","try_into","try_into","try_into","try_into","try_into","try_into","try_into","type_id","type_id","type_id","type_id","type_id","type_id","type_id","with_request","with_request_mut","with_response","with_response_mut"],"q":[[0,"interchange"]],"d":["The requester is building a request, using the …","The responder is building a response, using the …","The requester canceled the request. Responder needs to …","Channel used for Request/Response mechanism.","","The requester may send a new request.","Set of <code>N</code> channels","Interchange witout the <code>const N: usize</code> generic parameter …","The request is pending either processing by responder or …","Requester end of a channel","The responder sent a response.","Responder end of a channel","State of the RPC interchange","","Returns a reference to the interchange with the <code>N</code> …","","","","","","","","","","","","","","","Attempt to cancel a request.","Claim one of the channels of the interchange. Returns None …","Claim one of the channels of the interchange. Returns None …","","","","","","","","","","","Returns the argument unchanged.","Returns the argument unchanged.","Returns the argument unchanged.","Returns the argument unchanged.","Returns the argument unchanged.","Returns the argument unchanged.","","Returns the argument unchanged.","Calls <code>U::from(self)</code>.","Calls <code>U::from(self)</code>.","Calls <code>U::from(self)</code>.","Calls <code>U::from(self)</code>.","Calls <code>U::from(self)</code>.","Calls <code>U::from(self)</code>.","Calls <code>U::from(self)</code>.","","","Create a new Interchange","Send a request to the responder.","If there is a request waiting, obtain a reference to it","Initialize a request with its default values and and …","Obtain the requester end of the channel if it hasn’t …","Respond to a request.","Obtain the responder end of the channel if it hasn’t …","If there is a response waiting, obtain a reference to it","Initialize a response with its default values and and …","Send a request that was already placed in the channel …","Send a response that was already placed in the channel …","Obtain both the requester and responder ends of the …","Current state of the channel.","Current state of the channel.","If there is a request waiting, take a reference to it out","Look for a response. If the responder has sent a response, …","","","","","","","","","","","","","","","","","","","","","","If there is a request waiting, perform an operation with a …","Initialize a request with its default values and mutates …","If there is a request waiting, perform an operation with a …","Initialize a response with its default values and mutates …"],"i":[8,8,8,0,0,8,0,0,8,0,8,0,0,1,4,9,6,1,4,5,2,8,9,6,1,4,5,2,8,6,4,5,2,8,9,4,6,1,8,8,2,8,9,6,1,4,5,2,8,8,9,6,1,4,5,2,8,1,9,4,6,1,6,9,1,9,6,1,6,1,9,6,1,1,6,9,6,1,4,5,2,8,9,6,1,4,5,2,8,9,6,1,4,5,2,8,1,6,6,1],"f":[0,0,0,0,0,0,0,0,0,0,0,0,0,[1,[[3,[2]]]],[4,5],[[]],[[]],[[]],[[]],[[]],[[]],[[]],[[]],[[]],[[]],[[]],[[]],[[]],[[]],[6,[[3,[7,2]]]],[4,7],[5,7],[2,2],[8,8],[[],9],[[],4],[6],[1],[[8,10],11],[[8,8],11],[[2,12],13],[[8,12],13],[[]],[[]],[[]],[[]],[[]],[[]],[10,8],[[]],[[]],[[]],[[]],[[]],[[]],[[]],[[]],[1,11],[[],9],[[],4],[6,[[3,[2]]]],[1,[[3,[2]]]],[[[6,[14]]],[[3,[14,2]]]],[9,[[7,[6]]]],[1,[[3,[2]]]],[9,[[7,[1]]]],[6,[[3,[2]]]],[[[1,[14]]],[[3,[14,2]]]],[[[6,[14]]],[[3,[2]]]],[[[1,[14]]],[[3,[2]]]],[9,7],[6,8],[1,8],[1,7],[6,7],[[],3],[[],3],[[],3],[[],3],[[],3],[[],3],[[],3],[[],3],[[],3],[[],3],[[],3],[[],3],[[],3],[[],3],[[],15],[[],15],[[],15],[[],15],[[],15],[[],15],[[],15],[[1,16],[[3,[2]]]],[[[6,[14]],16],[[3,[2]]]],[[6,16],[[3,[2]]]],[[[1,[14]],16],[[3,[2]]]]],"c":[],"p":[[3,"Responder"],[3,"Error"],[4,"Result"],[3,"Interchange"],[3,"InterchangeRef"],[3,"Requester"],[4,"Option"],[4,"State"],[3,"Channel"],[15,"u8"],[15,"bool"],[3,"Formatter"],[6,"Result"],[8,"Default"],[3,"TypeId"],[8,"FnOnce"]]}\
}');
if (typeof window !== 'undefined' && window.initSearch) {window.initSearch(searchIndex)};
if (typeof exports !== 'undefined') {exports.searchIndex = searchIndex};
