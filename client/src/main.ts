import { initializeApp } from "firebase/app";
import { getFirestore, collection, addDoc, getDoc, doc, setDoc, onSnapshot, updateDoc } from "firebase/firestore";


// Your web app's Firebase configuration
const firebaseConfig = {
  apiKey: "AIzaSyCr12-OQV5bgdQPFoexd44O9Ubmht966pw",
  authDomain: "piperchat-2eacd.firebaseapp.com",
  projectId: "piperchat-2eacd",
  storageBucket: "piperchat-2eacd.appspot.com",
  messagingSenderId: "172730710087",
  appId: "1:172730710087:web:3dabdb9a62bee44e095962"
};

// Initialize Firebase
const app = initializeApp(firebaseConfig);
const db = getFirestore(app);


let peerConnection = new RTCPeerConnection({
  iceServers: [
    {
      urls: [
        'stun:stun.l.google.com:19302',
        'stun:stun1.l.google.com:19302',
        'stun:stun2.l.google.com:19302',
        'stun:stun3.l.google.com:19302',
        'stun:stun4.l.google.com:19302',
      ]
    },
    {
      urls: "turn:openrelay.metered.ca:80",
      username: "openrelayproject",
      credential: "openrelayproject",
    },
  ]
});

let localStream: MediaStream;
let remoteStream: MediaStream;

const webcamButton = document.getElementById('webcamButton');
const localVideo = document.getElementById('localVideo')! as HTMLVideoElement;
const remoteVideo = document.getElementById('remoteVideo')! as HTMLVideoElement;
const callButton = document.getElementById('callButton') as HTMLButtonElement;
const callInput = document.getElementById('callInput') as HTMLInputElement;
const answerButton = document.getElementById('answerButton') as HTMLButtonElement;


webcamButton?.addEventListener('click', async () => {
  if (localVideo.srcObject) {
    localVideo.srcObject = null;
    return;
  }

  localStream = await navigator.mediaDevices.getUserMedia({ video: true, audio: true, });
  webcamButton.innerHTML = 'Stop webcam';

  remoteStream = new MediaStream();
  remoteVideo.srcObject = remoteStream;


  localStream.getTracks().forEach((track) => { peerConnection.addTrack(track, localStream); });
  peerConnection.addEventListener('track', event => {
    event.streams[0].getTracks().forEach(track => {
      remoteStream.addTrack(track);
    });
  });

  localVideo.srcObject = localStream;
  remoteVideo.srcObject = remoteStream;

  callButton.disabled = false;
  answerButton.disabled = false;
});

callButton?.addEventListener('click', async () => {
  const callDoc = doc(collection(db, "calls"));
  const offerCandidates = collection(callDoc, "offerCandidates");
  const answerCandidates = collection(callDoc, "answerCandidates");

  callInput.value = callDoc.id;

  peerConnection.onicecandidate = (event) => {
    if (event.candidate) {
      addDoc(offerCandidates, event.candidate.toJSON());
    }
  }

  const offerDescription = await peerConnection.createOffer();
  await peerConnection.setLocalDescription(offerDescription);

  const offer = {
    sdp: offerDescription.sdp,
    type: offerDescription.type
  };

  await setDoc(callDoc, { offer });

  onSnapshot(callDoc, (snapshot) => {
    const data = snapshot.data();
    if (!peerConnection.currentRemoteDescription && data?.answer) {
      const answerDescription = new RTCSessionDescription(data.answer);
      peerConnection.setRemoteDescription(answerDescription);
    }
  });

  onSnapshot(answerCandidates, (snapshot) => {
    snapshot.docChanges().forEach((change) => {
      if (change.type === "added") {
        const candidate = new RTCIceCandidate(change.doc.data());
        peerConnection.addIceCandidate(candidate);
      }
    })
  })
});

answerButton?.addEventListener("click", async () => {
  const callId = callInput.value;
  const callDoc = doc(db, "calls", callId);
  const answerCandidates = collection(callDoc, "answerCandidates");
  const offerCandidates = collection(callDoc, "offerCandidates");

  peerConnection.onicecandidate = (event) => {
    if (event.candidate) {
      addDoc(answerCandidates, event.candidate.toJSON());
    }
  }

  const callData = (await getDoc(callDoc)).data();
  if (!callData) {
    console.error("Call document no longer exists");
    return;
  }
  const offerDescription = callData.offer;
  await peerConnection.setRemoteDescription(new RTCSessionDescription(offerDescription));

  const answerDescription = await peerConnection.createAnswer();
  await peerConnection.setLocalDescription(answerDescription);

  const answer = {
    type: answerDescription.type,
    sdp: answerDescription.sdp,
  };

  await updateDoc(callDoc, { answer });

  onSnapshot(offerCandidates, (snapshot) => {
    snapshot.docChanges().forEach((change) => {
      if (change.type === "added") {
        const data = change.doc.data();
        peerConnection.addIceCandidate(new RTCIceCandidate(data));
      }
    })
  });
});
