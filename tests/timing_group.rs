use juggle::utils::*;



#[test]
fn test_register(){
    let mut group = TimingGroup::<u32>::new();
    assert_eq!(group.count(),0);
    let k1 = group.insert(1);
    let k2 = group.insert(2);
    let k3 = group.insert(3);
    assert_eq!(group.count(),3);
    assert_eq!(group.get_slot_count(k1),Some(1));
    assert_eq!(group.get_slot_count(k2),Some(2));
    assert_eq!(group.get_slot_count(k3),Some(3));
    group.remove(k1);
    assert!(!group.contains(k1));
    assert_eq!(group.get_slot_count(k1),None);
    group.remove(k2);
    group.remove(k3);
    assert_eq!(group.count(),0);
}

#[test]
fn test_simple(){
    let mut group = TimingGroup::new();
    let k1 = group.insert(1);
    let k2 = group.insert(1);
    group.update_duration(k1,10);
    group.update_duration(k2,20);
    assert!(group.can_execute(k1) && !group.can_execute(k2));
    group.update_duration(k1,20);
    assert!(!group.can_execute(k1) && group.can_execute(k2));
    group.update_duration(k2,10);
    assert!(group.can_execute(k1) && group.can_execute(k2));
}