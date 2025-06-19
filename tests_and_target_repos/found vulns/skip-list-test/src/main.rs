struct Node<T> {
    item: Option<T>,
    next: Vec<*mut Node<T>>,
}

struct List<T> {
    head: *mut Node<T>,
}

impl<T> Node<T> {
    fn new(val: Option<T>) -> Self {
        Node::<T> {
            item: val,
            next: vec![],
        }
    }
}

impl<T> List<T> {
    fn new() -> Self {
        List::<T> {
            head: Box::into_raw(Box::new(Node::<T>::new(None))),
        }
    }
}

fn search(list: &List<u32>, key: u32) -> Option<u32> {
    let mut result = None;
    let mut node: *mut Node<u32> = (*list).head;
    for i in (0..3).rev() {
        unsafe {
            while i < (*node).next.len() && (*(*(node)).next[i]).item < Some(key) {
                node = (*(node)).next[i];
            }
            if i < (*node).next.len() && (*(*node).next[i]).item == Some(key) {
                result = Some(key);
                break;
            }
        }
    }
    result
}

fn main() {
    unsafe {
        let n3 = Box::into_raw(Box::new(Node::<u32>::new(Some(3))));
        let n6 = Box::into_raw(Box::new(Node::<u32>::new(Some(6))));
        let n7 = Box::into_raw(Box::new(Node::<u32>::new(Some(7))));
        let n9 = Box::into_raw(Box::new(Node::<u32>::new(Some(9))));
        let n12 = Box::into_raw(Box::new(Node::<u32>::new(Some(12))));
        let n17 = Box::into_raw(Box::new(Node::<u32>::new(Some(17))));
        let n19 = Box::into_raw(Box::new(Node::<u32>::new(Some(19))));
        let n21 = Box::into_raw(Box::new(Node::<u32>::new(Some(21))));
        let n25 = Box::into_raw(Box::new(Node::<u32>::new(Some(25))));
        let n26 = Box::into_raw(Box::new(Node::<u32>::new(Some(26))));

        //build levels
        let mut list = List::<u32>::new();
        (*(list.head)).next = vec![n3];
        let mut ptr = n3;
        let mut i = 0;
        let mut bk_ptr = list.head;
        while (*(*(*ptr).next)[0]).item != None {
            let item = (*(*(*ptr).next)[0]).item;
            ptr = (*(*ptr).next)[0];
        }

        //level 0
        (*n3).next.push(n6);
        (*n6).next.push(n7);
        (*n7).next.push(n9);
        (*n9).next.push(n12);
        (*n12).next.push(n17);
        (*n17).next.push(n19);
        (*n19).next.push(n21);
        (*n21).next.push(n25);
        (*n25).next.push(n26);

        //level1
        (*n6).next.push(n9);
        (*n9).next.push(n17);
        (*n17).next.push(n25);

        //level2
        (*n6).next.push(n25);

        let mut list = List::<u32>::new();
        (*(list.head)).next = vec![n3, n6, n6];

        assert!(search(&list, 0) == None);
        assert!(search(&list, 3) == Some(3));
        assert!(search(&list, 25) == Some(25));
        assert!(search(&list, 1) == Some(1));
        assert!(search(&list, 17) == Some(17));
        assert!(search(&list, 12) == Some(12));
        assert!(search(&list, 26) == Some(26));
    }
}
