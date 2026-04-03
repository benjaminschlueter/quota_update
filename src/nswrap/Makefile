XML_INCLUDE_PATH=/usr/include/libxml2

libnswrap.a: 
	gcc -g -I$(XML_INCLUDE_PATH) -I../marfs/src -I../scoutfs -c -fPIC nswrap.c -o libnswrap.a

clean:
	rm -f libnswrap.a
