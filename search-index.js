var searchIndex = JSON.parse('{\
"interchange":{"doc":"Implement a somewhat convenient and somewhat efficient way…","i":[[3,"Requester","interchange","Requesting end of the RPC interchange.",null,null],[3,"Responder","","Processing end of the RPC interchange.",null,null],[4,"State","","State of the RPC interchange",null,null],[13,"Idle","","The requester may send a new request.",0,null],[13,"Requested","","The request is pending either processing by responder or…",0,null],[13,"Processing","","The request is taken by responder, may still be…",0,null],[13,"Responded","","The responder sent a response.",0,null],[13,"Canceled","","The requester canceled the request. Responder needs to…",0,null],[8,"Interchange","","Do NOT implement this yourself! Use the macro…",null,null],[18,"CLIENT_CAPACITY","","",1,null],[16,"REQUEST","","",1,null],[16,"RESPONSE","","",1,null],[10,"claim","","This is the constructor for a `(Requester, Responder)` pair.",1,[[],["option",4]]],[10,"available_clients","","Method for debugging: how many allocated clients have not…",1,[[]]],[10,"reset_claims","","Method purely for testing - do not use in production",1,[[]]],[11,"state","","Current state of the interchange.",2,[[],["state",4]]],[11,"request","","Send a request to the responder.",2,[[],["result",4]]],[11,"cancel","","Attempt to cancel a request.",2,[[],[["option",4],["result",4]]]],[11,"take_response","","Look for a response.",2,[[],["option",4]]],[11,"state","","",3,[[],["state",4]]],[11,"take_request","","",3,[[],["option",4]]],[11,"is_canceled","","",3,[[]]],[11,"acknowledge_cancel","","",3,[[],["result",4]]],[11,"respond","","",3,[[],["result",4]]],[14,"interchange","","Use this macro to generate a pair of RPC pipes for any…",null,null],[11,"from","","",2,[[]]],[11,"borrow","","",2,[[]]],[11,"borrow_mut","","",2,[[]]],[11,"try_from","","",2,[[],["result",4]]],[11,"into","","",2,[[]]],[11,"try_into","","",2,[[],["result",4]]],[11,"type_id","","",2,[[],["typeid",3]]],[11,"from","","",3,[[]]],[11,"borrow","","",3,[[]]],[11,"borrow_mut","","",3,[[]]],[11,"try_from","","",3,[[],["result",4]]],[11,"into","","",3,[[]]],[11,"try_into","","",3,[[],["result",4]]],[11,"type_id","","",3,[[],["typeid",3]]],[11,"from","","",0,[[]]],[11,"borrow","","",0,[[]]],[11,"borrow_mut","","",0,[[]]],[11,"try_from","","",0,[[],["result",4]]],[11,"into","","",0,[[]]],[11,"try_into","","",0,[[],["result",4]]],[11,"type_id","","",0,[[],["typeid",3]]],[11,"from","","",0,[[]]],[11,"fmt","","",0,[[["formatter",3]],["result",6]]],[11,"eq","","",0,[[["state",4]]]],[11,"eq","","",0,[[]]],[11,"clone","","",0,[[],["state",4]]]],"p":[[4,"State"],[8,"Interchange"],[3,"Requester"],[3,"Responder"]]}\
}');
addSearchOptions(searchIndex);initSearch(searchIndex);